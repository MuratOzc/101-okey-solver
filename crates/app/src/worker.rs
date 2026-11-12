use okey_engine::analysis::{Analyzer, Options, Report, View};
use okey_engine::net::Net;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::sync::Arc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent};

#[derive(Deserialize)]
#[serde(tag = "kind")]
enum Req {
    Start {
        generation: u32,
        view: View,
        seed: u64,
        #[serde(default)]
        options: Options,
        #[serde(default)]
        model: bool,
    },
    Stop,
}

#[derive(Serialize)]
struct Resp {
    generation: u32,
    report: Option<Report>,
    error: Option<String>,
    done: bool,
    ms: f64,
}

struct Job {
    generation: u32,
    an: Analyzer,
    batch: usize,
    started: f64,
}

thread_local! {
    static JOB: RefCell<Option<Job>> = const { RefCell::new(None) };
    static NET: Option<Arc<Net>> = network(include_bytes!("../model/value.bin"));
    static TEAM_NET: Option<Arc<Net>> = network(include_bytes!("../model/partners.bin"));
}

fn network(bytes: &[u8]) -> Option<Arc<Net>> {
    let mut net = Net::from_bytes(bytes).ok()?;
    net.discards = PLAY_ON_DISCARDS;
    Some(Arc::new(net))
}

const PLAY_ON_DISCARDS: usize = 5;

const MAX_ITERATIONS: u64 = 4000;
const MAX_MS: f64 = 60_000.0;
const TARGET_TICK_MS: f64 = 150.0;

fn scope() -> DedicatedWorkerGlobalScope {
    js_sys::global().unchecked_into()
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn post(r: &Resp) {
    if let Ok(s) = serde_json::to_string(r) {
        let _ = scope().post_message(&JsValue::from_str(&s));
    }
}

fn schedule(generation: u32) {
    let cb = Closure::once_into_js(move || tick(generation));
    let _ = scope().set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), 0);
}

fn tick(generation: u32) {
    let resp = JOB.with(|j| {
        let mut j = j.borrow_mut();
        let job = j.as_mut().filter(|job| job.generation == generation)?;
        let t0 = now();
        job.an.step(job.batch);
        let dt = now() - t0;
        if dt < TARGET_TICK_MS / 2.0 {
            job.batch = (job.batch * 2).min(4096);
        } else if dt > TARGET_TICK_MS * 2.0 && job.batch > 1 {
            job.batch /= 2;
        }
        let report = job.an.report();
        let ms = now() - job.started;
        let done = report.iterations >= MAX_ITERATIONS || ms > MAX_MS;
        Some(Resp {
            generation,
            report: Some(report),
            error: None,
            done,
            ms,
        })
    });
    if let Some(r) = resp {
        post(&r);
        if !r.done {
            schedule(generation);
        }
    }
}

fn main() {
    console_error_panic_hook::set_once();
    let onmsg = Closure::<dyn FnMut(MessageEvent)>::new(|e: MessageEvent| {
        let Some(s) = e.data().as_string() else { return };
        match serde_json::from_str::<Req>(&s) {
            Ok(Req::Start {
                generation,
                view,
                seed,
                options,
                model,
            }) => match Analyzer::with_options(view, seed, options) {
                Ok(mut an) => {
                    for net in [&NET, &TEAM_NET] {
                        if model && let Some(net) = net.with(|net| net.clone()) {
                            an.use_model(net, 0.0, true);
                        }
                    }
                    JOB.with(|j| {
                        *j.borrow_mut() = Some(Job {
                            generation,
                            an,
                            batch: 2,
                            started: now(),
                        })
                    });
                    schedule(generation);
                }
                Err(error) => {
                    JOB.with(|j| *j.borrow_mut() = None);
                    post(&Resp {
                        generation,
                        report: None,
                        error: Some(error),
                        done: true,
                        ms: 0.0,
                    });
                }
            },
            Ok(Req::Stop) => JOB.with(|j| *j.borrow_mut() = None),
            Err(err) => post(&Resp {
                generation: 0,
                report: None,
                error: Some(err.to_string()),
                done: true,
                ms: 0.0,
            }),
        }
    });
    scope().set_onmessage(Some(onmsg.as_ref().unchecked_ref()));
    onmsg.forget();
}
