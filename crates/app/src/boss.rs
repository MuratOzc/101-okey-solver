//! The analyzer that keeps the statistics, and the workers that play its deals.

use super::*;

#[derive(Clone, Debug)]
pub(super) struct Resp {
    pub(super) report: Option<Report>,
    pub(super) error: Option<String>,
    pub(super) done: bool,
}

#[derive(Deserialize)]
pub(super) struct Played {
    pub(super) generation: u32,
    pub(super) version: u32,
    pub(super) deals: Vec<Vec<Packed>>,
    pub(super) error: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind")]
pub(super) enum Req<'a> {
    Start {
        generation: u32,
        view: &'a okey_engine::analysis::View,
        seed: u64,
        model: bool,
        field: &'a Field,
    },
    Field {
        generation: u32,
        field: &'a Field,
    },
    Stop,
}

/// The one analyzer that keeps the statistics; the workers only play the deals it asks for.
pub(super) struct Boss {
    pub(super) generation: u32,
    pub(super) an: Analyzer,
    pub(super) view: okey_engine::analysis::View,
    pub(super) model: bool,
    pub(super) base: u64,
    pub(super) sent: u32,
    pub(super) started: f64,
    pub(super) shown_at: f64,
    pub(super) shown: Option<Decision>,
    pub(super) done: bool,
}

pub(super) const MAX_DEALS: u64 = 40_000;
pub(super) const MAX_MS: f64 = 60_000.0;
pub(super) const SHOW_EVERY_MS: f64 = 120.0;

pub(super) const MAX_WORKERS: usize = 12;

/// worker.js and worker_bg.wasm keep their names across builds; without the build id a cache can pair old glue with new wasm.
pub(super) fn worker_url() -> Option<String> {
    let href = web_sys::window()?.location().href().ok()?;
    let base = &href[..href.rfind('/')? + 1];
    let id = env!("BUILD_ID");
    let src = format!("importScripts('{base}worker.js?v={id}');wasm_bindgen('{base}worker_bg.wasm?v={id}');");
    let parts = js_sys::Array::of1(&JsValue::from_str(&src));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("text/javascript");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts).ok()?;
    web_sys::Url::create_object_url_with_blob(&blob).ok()
}

pub(super) fn worker_count() -> usize {
    let cores = web_sys::window().map_or(2.0, |w| w.navigator().hardware_concurrency()) as usize;
    cores.saturating_sub(1).clamp(1, MAX_WORKERS)
}

pub(super) fn send_all(make: impl Fn(usize) -> Option<String>) {
    WORKERS.with(|ws| {
        for (i, w) in ws.borrow().iter().enumerate() {
            if let Some(j) = make(i) {
                let _ = w.post_message(&JsValue::from_str(&j));
            }
        }
    });
}

pub(super) fn stop_all() {
    send_all(|_| serde_json::to_string(&Req::Stop).ok());
}

pub(super) fn start_msg(boss: &Boss, field: &Field, stream: usize) -> Option<String> {
    serde_json::to_string(&Req::Start {
        generation: boss.generation,
        view: &boss.view,
        seed: boss.base.wrapping_add((stream as u64 + 1) * 0x9E37_79B9),
        model: boss.model,
        field,
    })
    .ok()
}

pub(super) fn start_all(generation: u32, view: &okey_engine::analysis::View, model: bool) -> Result<(), String> {
    let now = js_sys::Date::now();
    let base = now as u64;
    let mut an = Analyzer::new(view.clone(), base)?;
    let field = an.field();
    let boss = Boss {
        generation,
        an,
        view: view.clone(),
        model,
        base,
        sent: field.version,
        started: now,
        shown_at: 0.0,
        shown: None,
        done: false,
    };
    send_all(|i| start_msg(&boss, &field, i));
    BOSS.with(|b| *b.borrow_mut() = Some(boss));
    Ok(())
}

// A worker takes messages only once its wasm is loaded, so a start sent before that never reached it.
pub(super) fn start_late(i: usize) {
    BOSS.with(|b| {
        let mut b = b.borrow_mut();
        let Some(boss) = b.as_mut().filter(|boss| !boss.done) else { return };
        let field = boss.an.field();
        WORKERS.with(|ws| {
            let ws = ws.borrow();
            // Its own stream of deals, in case the first start did arrive and those deals are already counted.
            if let (Some(w), Some(j)) = (ws.get(i), start_msg(boss, &field, ws.len() + i)) {
                let _ = w.post_message(&JsValue::from_str(&j));
            }
        });
    });
}

/// Keep the recommendation on screen while it is within the noise of the new leader.
pub(super) fn steady(report: &mut Report, shown: &mut Option<Decision>) {
    if let Some(d) = *shown
        && let Some(i) = report.candidates.iter().position(|c| c.decision == d && c.alive)
        && i > 0
    {
        let (c, top) = (&report.candidates[i], &report.candidates[0]);
        let usable = c.mv.is_some() || !matches!(c.decision, Decision::Play(_));
        if usable && c.mean - top.mean <= c.diff_se {
            let c = report.candidates.remove(i);
            report.candidates.insert(0, c);
        }
    }
    *shown = report.candidates.first().map(|c| c.decision);
}

/// Take in what a worker played; now and then, a fresh report for the screen.
pub(super) fn played(msg: Played) -> Option<Resp> {
    BOSS.with(|b| {
        let mut b = b.borrow_mut();
        let boss = b.as_mut().filter(|boss| boss.generation == msg.generation && !boss.done)?;
        if let Some(e) = msg.error {
            boss.done = true;
            return Some(Resp {
                report: None,
                error: Some(e),
                done: true,
            });
        }
        boss.an.absorb(msg.version, &msg.deals);
        if boss.an.version() != boss.sent {
            let field = boss.an.field();
            boss.sent = field.version;
            let generation = boss.generation;
            send_all(|_| serde_json::to_string(&Req::Field { generation, field: &field }).ok());
        }
        let now = js_sys::Date::now();
        if now - boss.shown_at < SHOW_EVERY_MS {
            return None;
        }
        let mut report = boss.an.report();
        if report.candidates.first().is_none_or(|c| c.n == 0) {
            return None;
        }
        boss.shown_at = now;
        boss.done = report.settled || report.iterations >= MAX_DEALS || now - boss.started > MAX_MS;
        if boss.done {
            stop_all();
        }
        steady(&mut report, &mut boss.shown);
        Some(Resp {
            report: Some(report),
            error: None,
            done: boss.done,
        })
    })
}
