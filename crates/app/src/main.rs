mod settings;
mod state;

use leptos::prelude::*;
use okey_engine::analysis::{
    Analyzer, CandReport, Decision, Field, HandReport, Options, Packed, Phase, Report, View, describe, describe_move, hand_report,
    model_fits,
};
use okey_engine::game::{Opened, Step};
use okey_engine::meld::{Shape, pos_tile};
use okey_engine::moves::{Act, Move};
use okey_engine::reads::{BAND, MAX_PAIRS, Read, read_opponents};
use okey_engine::table::{JokerSlot, LayOption, Side};
use okey_engine::tile::*;
use serde::{Deserialize, Serialize};
use state::{Mode, SEAT_NAMES, Stage, State};
use std::cell::RefCell;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{MessageEvent, Worker};

const STORAGE_KEY: &str = "okey101.state.v2";
const UNDO_LIMIT: usize = 300;
const SEAT_OWNER: [&str; 4] = ["kendi", "Sağ'ın", "Karşı'nın", "Sol'un"];
const OPPONENTS: [u8; 3] = [3, 2, 1];
const READ_DEALS: usize = 200;
const GROUP_COLORS: [&str; 6] = ["#22c55e", "#38bdf8", "#f59e0b", "#e879f9", "#f87171", "#a3e635"];

#[derive(Clone, Debug)]
struct Resp {
    report: Option<Report>,
    error: Option<String>,
    done: bool,
}

#[derive(Deserialize)]
struct Played {
    generation: u32,
    version: u32,
    deals: Vec<Vec<Packed>>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind")]
enum Req<'a> {
    Start {
        generation: u32,
        view: &'a okey_engine::analysis::View,
        seed: u64,
        options: Options,
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
struct Boss {
    generation: u32,
    an: Analyzer,
    view: okey_engine::analysis::View,
    model: bool,
    base: u64,
    sent: u32,
    started: f64,
    shown_at: f64,
    shown: Option<Decision>,
    done: bool,
}

const MAX_DEALS: u64 = 40_000;
const MAX_MS: f64 = 60_000.0;
const SHOW_EVERY_MS: f64 = 120.0;

thread_local! {
    static WORKERS: RefCell<Vec<Worker>> = const { RefCell::new(Vec::new()) };
    static BOSS: RefCell<Option<Boss>> = const { RefCell::new(None) };
    static LAST_SENT: RefCell<String> = const { RefCell::new(String::new()) };
    static GENERATION: RefCell<u32> = const { RefCell::new(0) };
    static UNDO: RefCell<Vec<State>> = const { RefCell::new(Vec::new()) };
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn load_state() -> State {
    storage()
        .and_then(|s| s.get_item(STORAGE_KEY).ok().flatten())
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default()
}

fn save_state(s: &State) {
    if let (Some(st), Ok(j)) = (storage(), serde_json::to_string(s)) {
        let _ = st.set_item(STORAGE_KEY, &j);
    }
}

const MAX_WORKERS: usize = 12;

fn worker_count() -> usize {
    let cores = web_sys::window().map_or(2.0, |w| w.navigator().hardware_concurrency()) as usize;
    cores.saturating_sub(1).clamp(1, MAX_WORKERS)
}

fn send_all(make: impl Fn(usize) -> Option<String>) {
    WORKERS.with(|ws| {
        for (i, w) in ws.borrow().iter().enumerate() {
            if let Some(j) = make(i) {
                let _ = w.post_message(&JsValue::from_str(&j));
            }
        }
    });
}

fn stop_all() {
    send_all(|_| serde_json::to_string(&Req::Stop).ok());
}

fn start_msg(boss: &Boss, field: &Field, stream: usize) -> Option<String> {
    serde_json::to_string(&Req::Start {
        generation: boss.generation,
        view: &boss.view,
        seed: boss.base.wrapping_add((stream as u64 + 1) * 0x9E37_79B9),
        options: Options::default(),
        model: boss.model,
        field,
    })
    .ok()
}

fn start_all(generation: u32, view: &okey_engine::analysis::View, model: bool) -> Result<(), String> {
    let now = js_sys::Date::now();
    let base = now as u64;
    let mut an = Analyzer::with_options(view.clone(), base, Options::default())?;
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
fn start_late(i: usize) {
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
fn steady(report: &mut Report, shown: &mut Option<Decision>) {
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
fn played(msg: Played) -> Option<Resp> {
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

type Choice = (Decision, Option<Move>);

#[derive(Clone, Copy)]
pub struct Ctx {
    pub st: RwSignal<State>,
    msg: RwSignal<Option<String>>,
    analysis: RwSignal<Option<Resp>>,
    pub show_settings: RwSignal<bool>,
    show_info: RwSignal<bool>,
    pick: RwSignal<Option<Choice>>,
    best: Memo<Option<Choice>>,
    chosen: Memo<Option<Choice>>,
    steps: Memo<Option<Vec<Step>>>,
    hand: Memo<Option<HandReport>>,
    reads: Memo<Option<Vec<Read>>>,
}

impl Ctx {
    pub fn act(&self, f: impl FnOnce(&mut State) -> Result<(), String>) {
        let mut s = self.st.get_untracked();
        let before = s.clone();
        match f(&mut s) {
            Ok(()) => {
                UNDO.with(|u| {
                    let mut u = u.borrow_mut();
                    u.push(before);
                    if u.len() > UNDO_LIMIT {
                        u.remove(0);
                    }
                });
                save_state(&s);
                self.st.set(s);
                self.msg.set(None);
            }
            Err(e) => self.msg.set(Some(e)),
        }
    }

    fn undo(&self) {
        if let Some(prev) = UNDO.with(|u| u.borrow_mut().pop()) {
            save_state(&prev);
            self.st.set(prev);
            self.msg.set(None);
        }
    }

    fn mode(&self, m: Mode) {
        self.act(|s| {
            s.buffer.clear();
            s.mode = m;
            Ok(())
        })
    }
}

fn color_class(t: Tile) -> &'static str {
    if t == JOKER {
        "ok"
    } else {
        ["k", "s", "m", "si"][color(t) as usize]
    }
}

fn tile_view(t: Tile, okey: Option<Tile>) -> impl IntoView + use<> {
    let sahte = t != JOKER && Some(t) == okey;
    let title = if t == JOKER {
        "Okey".to_string()
    } else if sahte {
        format!("Sahte okey ({} yerine)", name(t))
    } else {
        name(t)
    };
    view! {
        <span class=format!("t {}", color_class(t)) class:sahte=sahte title=title>
            {if t == JOKER { "★".to_string() } else { num(t).to_string() }}
        </span>
    }
}

fn tiles_view(tiles: &[Tile], okey: Option<Tile>) -> impl IntoView + use<> {
    tiles.iter().map(|&t| tile_view(t, okey)).collect_view()
}

fn okey_tile(s: &State) -> Option<Tile> {
    s.gosterge.map(okey_of)
}

fn seat_name(s: &State, p: u8) -> String {
    if s.rules.partners && p == 2 {
        "Karşı (eş)".into()
    } else {
        SEAT_NAMES[p as usize].into()
    }
}

fn shape_label(sh: &Shape) -> &'static str {
    match sh {
        Shape::Run { .. } => "Seri",
        Shape::Group { .. } => "Grup",
        _ => "Çift",
    }
}

fn side_text(side: Side) -> &'static str {
    match side {
        Side::Low => "soluna",
        Side::High => "sağına",
        Side::Group => "yanına",
    }
}

fn okey_note(sh: &Shape) -> Option<String> {
    match *sh {
        Shape::Run { color, lo, hi, jokers } if jokers != 0 => {
            let names: Vec<String> = (lo..=hi)
                .filter(|p| jokers >> p & 1 == 1)
                .map(|p| name(pos_tile(color, p)))
                .collect();
            Some(format!("okey = {}", names.join(", ")))
        }
        Shape::Group { jokers, .. } if jokers > 0 => Some("okey = eksik renk".into()),
        _ => None,
    }
}

fn pct(x: f64) -> String {
    format!("%{:.0}", x * 100.0)
}

// Only some simulated deals are played to the end of the hand; fewer than this say nothing about how it ends.
const ENOUGH_ENDINGS: usize = 12;

fn chance(x: f64) -> String {
    match x * 100.0 {
        p if !(1.0..=99.0).contains(&p) => format!("%{p:.2}"),
        p if !(10.0..=90.0).contains(&p) => format!("%{p:.1}"),
        p => format!("%{p:.0}"),
    }
}

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let st = RwSignal::new(load_state());
    let analysis: RwSignal<Option<Resp>> = RwSignal::new(None);
    let pick: RwSignal<Option<Choice>> = RwSignal::new(None);
    let best = Memo::new(move |_| {
        analysis.with(|a| {
            a.as_ref()
                .and_then(|r| r.report.as_ref())
                .and_then(|r| r.candidates.first().map(|c| (c.decision, c.mv.clone())))
        })
    });
    let chosen = Memo::new(move |_| pick.get().or(best.get()));
    let steps = Memo::new(move |_| {
        let (d, mv) = chosen.get()?;
        if matches!(d, Decision::Draw | Decision::Take) {
            return None;
        }
        let v = st.with(|s| s.view())?;
        Some(match &mv {
            Some(m) => describe_move(&v, m),
            None => describe(&v, d),
        })
    });
    let view_key = Memo::new(move |_| {
        st.with(|s| {
            (s.stage == Stage::Play)
                .then(|| s.view_as(Phase::AfterDraw))
                .flatten()
                .and_then(|v| serde_json::to_string(&v).ok())
        })
    });
    let ctx = Ctx {
        st,
        msg: RwSignal::new(None),
        analysis,
        show_settings: RwSignal::new(false),
        show_info: RwSignal::new(false),
        pick,
        best,
        chosen,
        steps,
        hand: Memo::new(move |_| {
            st.with(|s| {
                (s.stage == Stage::Play)
                    .then(|| s.view_as(Phase::AfterDraw).map(|v| hand_report(&v)))
                    .flatten()
            })
        }),
        reads: Memo::new(move |_| {
            view_key
                .get()
                .and_then(|j| serde_json::from_str::<View>(&j).ok())
                .map(|v| read_opponents(&v, READ_DEALS, 11))
        }),
    };
    provide_context(ctx);

    for i in 0..worker_count() {
        let Ok(w) = Worker::new("./worker_loader.js") else { break };
        let onmsg = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            let Some(s) = e.data().as_string() else { return };
            if s == "ready" {
                start_late(i);
                return;
            }
            let Ok(msg) = serde_json::from_str::<Played>(&s) else { return };
            if let Some(r) = played(msg) {
                analysis.set(Some(r));
            }
        });
        w.set_onmessage(Some(onmsg.as_ref().unchecked_ref()));
        onmsg.forget();
        WORKERS.with(|x| x.borrow_mut().push(w));
    }

    Effect::new(move |_| {
        let (view, model) = st.with(|s| (s.view(), s.model));
        let key = view
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok())
            .map(|v| format!("{model}{v}"))
            .unwrap_or_default();
        if LAST_SENT.with(|l| *l.borrow() == key) {
            return;
        }
        LAST_SENT.with(|l| *l.borrow_mut() = key);
        let generation = GENERATION.with(|g| {
            *g.borrow_mut() += 1;
            *g.borrow()
        });
        analysis.set(None);
        pick.set(None);
        BOSS.with(|b| *b.borrow_mut() = None);
        match view {
            Some(v) => {
                if let Err(e) = start_all(generation, &v, model) {
                    stop_all();
                    analysis.set(Some(Resp {
                        report: None,
                        error: Some(e),
                        done: true,
                    }));
                }
            }
            None => stop_all(),
        }
    });

    view! {
        <div class="app">
            <TopBar/>
            <Show when=move || st.with(|s| s.stage != Stage::Setup)>
                <Seats/>
            </Show>
            <TableZone/>
            <div class="bottom">
                {move || if st.with(|s| s.keyboard_visible()) {
                    view! { <Picker/> }.into_any()
                } else if st.with(|s| s.stage == Stage::Setup) {
                    ().into_any()
                } else {
                    view! { <Rack/> }.into_any()
                }}
            </div>
            <aside class="panel">
                <Message/>
                <Panel/>
            </aside>
            <Show when=move || ctx.show_settings.get()>
                <Modal title="Ayarlar" open=ctx.show_settings>
                    <settings::Basics/>
                    <h3>"Maç"</h3>
                    <MatchBox/>
                    <settings::Advanced/>
                </Modal>
            </Show>
            <Show when=move || ctx.show_info.get()>
                <Modal title="Taşlar" open=ctx.show_info>
                    <Info/>
                </Modal>
            </Show>
        </div>
    }
}

#[component]
fn Modal(title: &'static str, open: RwSignal<bool>, children: Children) -> impl IntoView {
    view! {
        <div class="scrim" on:click=move |_| open.set(false)>
            <div class="modal" on:click=|e| e.stop_propagation()>
                <div class="modal-head">
                    <h2>{title}</h2>
                    <button class="btn light" on:click=move |_| open.set(false)>"Kapat"</button>
                </div>
                <div class="modal-body">{children()}</div>
            </div>
        </div>
    }
}

#[component]
fn TopBar() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    view! {
        <header class="top">
            {move || {
                let s = st.get();
                let deck = (s.stage == Stage::Play).then(|| view! { <span class="chip">"Deste " <b>{s.deck_count}</b></span> });
                let gost = s.gosterge.filter(|_| s.stage != Stage::Setup).map(|g| view! {
                    <span class="chip">"Gösterge " {tile_view(g, None)} " okey " {tile_view(okey_of(g), None)}</span>
                });
                let head = match s.stage {
                    Stage::Setup => Some("Yeni el"),
                    Stage::Over => Some("El bitti"),
                    Stage::Play => None,
                }
                .map(|t| view! { <span class="chip">{t}</span> });
                let game = (s.stage != Stage::Setup).then(|| view! {
                    <button class="chip link" on:click=move |_| ctx.show_settings.set(true)>{format!("El {}/{}", s.hand_no, s.hands_total)}{standing_short(&s)}</button>
                });
                view! { {head} {deck} {gost} {game} }
            }}
            <span class="grow"></span>
            <button class="icon" title="Geri al" on:click=move |_| ctx.undo()>"↶"</button>
            <Show when=move || st.with(|s| s.stage != Stage::Setup)>
                <button class="icon" title="Taşlar: atılanlar, görülmemişler" on:click=move |_| ctx.show_info.set(true)>"☷"</button>
            </Show>
            <button class="icon" title="Ayarlar" on:click=move |_| ctx.show_settings.set(true)>"⚙"</button>
        </header>
    }
}

#[component]
fn Seats() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    view! {
        <div class="seats">
            {move || {
                let s = st.get();
                let okey = okey_tile(&s);
                OPPONENTS.iter().map(|&p| {
                    let pv = &s.players[p as usize];
                    let count = pv.hand_len.unwrap_or(0);
                    let badge = match pv.opened {
                        Opened::No => None,
                        Opened::Melds => Some(format!("açtı {}", s.total(pv.open_value))),
                        Opened::Pairs => Some(format!("{} çift", pv.open_value)),
                    };
                    let cur = s.stage == Stage::Play && s.turn == p;
                    let takeable = p == 3 && s.stage == Stage::Play && s.turn == 0 && !s.may_act();
                    let name = seat_name(&s, p);
                    let initials: String = SEAT_NAMES[p as usize].chars().take(2).collect();
                    let guess = ctx.reads.get().and_then(|rs| rs.into_iter().find(|r| r.seat == p)).map(|r| read_line(&r, &s));
                    view! {
                        <div class="seat" class:cur=cur>
                            <span class="av">{initials}</span>
                            <span class="who">
                                <b>{name}</b>
                                <small>{format!("{count} taş")} {badge.map(|b| view! { " · " <span class="open">{b}</span> })}</small>
                                {guess.map(|g| view! { <small class="guess" title="Tahmini eli">{g}</small> })}
                            </span>
                            <span class="last" class:take=takeable title="Son attığı">
                                {pv.discards.last().map(|&t| tile_view(t, okey))}
                            </span>
                        </div>
                    }
                }).collect_view()
            }}
        </div>
    }
}

#[component]
fn TableZone() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    view! {
        <section class="tablezone">
            {move || {
                let s = st.get();
                let okey = okey_tile(&s);
                if s.stage == Stage::Setup {
                    return setup_table(ctx, &s).into_any();
                }
                let pending = (s.mode == Mode::MeldEntry && !s.buffer.is_empty()).then(|| s.buffer.clone());
                let owners: Vec<u8> = [3u8, 2, 1, 0]
                    .into_iter()
                    .filter(|&o| s.table.melds.iter().any(|m| m.owner == o) || (pending.is_some() && o == s.turn))
                    .collect();
                if owners.is_empty() {
                    return view! { <div class="empty">"Henüz açan yok"</div> }.into_any();
                }
                let menu = match s.mode {
                    Mode::MeldMenu(i) => Some(i),
                    _ => None,
                };
                owners.into_iter().map(|o| {
                    let melds: Vec<_> = s.table.melds.iter().enumerate()
                        .filter(|(_, m)| m.owner == o)
                        .map(|(i, m)| (i, m.shape.tiles(), s.meld_tappable(i)))
                        .collect();
                    let ghost = pending.clone().filter(|_| o == s.turn);
                    view! {
                        <div class="owner">
                            <span class="oname">{seat_name(&s, o)}</span>
                            <div class="melds">
                                {melds.into_iter().map(|(i, tiles, tap)| view! {
                                    <button class="grp" class:tap=tap class:on={menu == Some(i)} disabled=!tap
                                        on:click=move |_| ctx.act(|s| { s.buffer.clear(); s.mode = Mode::MeldMenu(i); Ok(()) })>
                                        {tiles_view(&tiles, okey)}
                                    </button>
                                }).collect_view()}
                                {ghost.map(|g| view! { <span class="grp ghost">{tiles_view(&g, okey)}</span> })}
                            </div>
                        </div>
                    }
                }).collect_view().into_any()
            }}
        </section>
    }
}

fn setup_table(ctx: Ctx, s: &State) -> impl IntoView + use<> {
    let okey = okey_tile(s);
    let gost = s.gosterge.map(|g| {
        view! {
            <div class="gost">
                <span class="glabel">"Gösterge"</span>
                <span class="big-tile">{tile_view(g, None)}</span>
                <span class="glabel">"okey " {tile_view(okey_of(g), None)}</span>
            </div>
        }
    });
    let hand = (s.mode == Mode::Hand).then(|| {
        let tiles = s.hand.clone();
        view! {
            <div class="entered">
                {tiles.into_iter().enumerate().map(|(i, t)| view! {
                    <button class="tbtn" title="Sil" on:click=move |_| ctx.act(|s| { s.remove_from_hand(i); Ok(()) })>{tile_view(t, okey)}</button>
                }).collect_view()}
            </div>
        }
    });
    view! { <div class="setup">{gost}{hand}</div> }
}

#[derive(Clone, Copy, PartialEq)]
enum Mark {
    Plain,
    Group(usize),
    Lay(usize),
    Swap(usize),
    Throw,
}

fn take_tile(rest: &mut Vec<Tile>, t: Tile) -> bool {
    match rest.iter().position(|&x| x == t) {
        Some(i) => {
            rest.remove(i);
            true
        }
        None => false,
    }
}

fn arrange(hand: &[Tile], steps: Option<&[Step]>) -> Vec<Vec<(Tile, Mark)>> {
    let mut rest = hand.to_vec();
    let mut chunks: Vec<Vec<(Tile, Mark)>> = Vec::new();
    let mut n = 0;
    for step in steps.unwrap_or_default() {
        let (tiles, mark) = match step {
            Step::Meld { shape, .. } => {
                n += 1;
                (shape.tiles(), Mark::Group(n))
            }
            Step::Layoff { tile, .. } => {
                n += 1;
                (vec![*tile], Mark::Lay(n))
            }
            Step::Swap { give, .. } => {
                n += 1;
                (give.clone(), Mark::Swap(n))
            }
            Step::Discard { tile, .. } => (vec![*tile], Mark::Throw),
        };
        let chunk: Vec<(Tile, Mark)> = tiles.into_iter().filter(|&t| take_tile(&mut rest, t)).map(|t| (t, mark)).collect();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }
    }
    let rest_start = chunks.len();
    rest.sort_unstable();
    for t in rest {
        let key = if t == JOKER { 9 } else { color(t) };
        let same = chunks.len() > rest_start
            && chunks
                .last()
                .and_then(|c| c.last())
                .is_some_and(|&(p, _)| if p == JOKER { 9 } else { color(p) } == key);
        if same {
            chunks.last_mut().unwrap().push((t, Mark::Plain));
        } else {
            chunks.push(vec![(t, Mark::Plain)]);
        }
    }
    chunks
}

fn split_shelves(chunks: Vec<Vec<(Tile, Mark)>>) -> [Vec<Vec<(Tile, Mark)>>; 2] {
    let total: usize = chunks.iter().map(|c| c.len()).sum();
    let half = total.div_ceil(2);
    let (mut a, mut b) = (Vec::new(), Vec::new());
    let mut count = 0;
    for c in chunks {
        let fits = b.is_empty() && (a.is_empty() || (count < half && count + c.len() <= 15));
        if fits {
            count += c.len();
            a.push(c);
        } else {
            b.push(c);
        }
    }
    [a, b]
}

#[component]
fn Rack() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    view! {
        <div class="rack">
            {move || {
                let s = st.get();
                let okey = okey_tile(&s);
                let me = s.turn == 0 && s.stage == Stage::Play;
                let active = me && s.may_act();
                let planning = active && s.mode == Mode::Idle && s.opening_complete();
                let steps = if planning { ctx.steps.get() } else { None };
                let mut used = if s.mode == Mode::Manual { s.buffer.clone() } else { vec![] };
                let pv = &s.players[0];
                let opened = match pv.opened {
                    Opened::No => None,
                    Opened::Melds => Some(format!("açtın {}", s.total(pv.open_value))),
                    Opened::Pairs => Some(format!("{} çift açtın", pv.open_value)),
                };
                let head = view! {
                    <div class="rack-head" class:cur=me>
                        <b>"Ben"</b>
                        <span>{format!("{} taş", s.hand.len())}</span>
                        {opened.map(|o| view! { <span class="open">{o}</span> })}
                        {ctx.hand.get().filter(|_| pv.opened == Opened::No).map(|h| view! {
                            <span class="reach" title="Elindeki taşlarla en iyi dizilişte">
                                <span class:open={h.open_value >= h.meld_req}>{format!("per {} / {}", s.total(h.open_value), s.total(h.meld_req))}</span>
                                " · "
                                <span class:open={h.pairs >= h.pair_req}>{format!("çift {} / {}", h.pairs, h.pair_req)}</span>
                            </span>
                        })}
                        <span class="grow"></span>
                        {pv.discards.last().map(|&t| view! { <span class="mine">"attığım " {tile_view(t, okey)}</span> })}
                    </div>
                };
                let shelves = split_shelves(arrange(&s.hand, steps.as_deref()));
                let shelves = shelves.into_iter().map(|chunks| {
                    let chunks = chunks.into_iter().map(|chunk| {
                        chunk.into_iter().enumerate().map(|(i, (t, mark))| {
                            let sel = used.iter().position(|&b| b == t).map(|k| used.remove(k)).is_some();
                            let (style, badge) = match mark {
                                Mark::Group(n) => (format!("--mark:{}", GROUP_COLORS[(n - 1) % GROUP_COLORS.len()]), (i == 0).then(|| n.to_string())),
                                Mark::Lay(n) => ("--mark:#ffffff".to_string(), Some(n.to_string())),
                                Mark::Swap(n) => ("--mark:#c084fc".to_string(), Some(n.to_string())),
                                Mark::Throw => ("--mark:#ef4444".to_string(), Some("AT".to_string())),
                                Mark::Plain => (String::new(), None),
                            };
                            view! {
                                <button class="tbtn" class:marked={mark != Mark::Plain} class:throw={mark == Mark::Throw} class:sel=sel style=style disabled=!active
                                    on:click=move |_| ctx.act(|s| {
                                        if s.mode != Mode::Manual {
                                            s.buffer.clear();
                                            s.mode = Mode::Manual;
                                        }
                                        if sel {
                                            if let Some(i) = s.buffer.iter().rposition(|&b| b == t) { s.buffer.remove(i); }
                                        } else {
                                            s.buffer.push(t);
                                        }
                                        Ok(())
                                    })>
                                    {tile_view(t, okey)}
                                    {badge.map(|b| view! { <i class="badge">{b}</i> })}
                                </button>
                            }
                        }).collect_view()
                    }).collect::<Vec<_>>();
                    let mut parts = Vec::new();
                    for (k, c) in chunks.into_iter().enumerate() {
                        if k > 0 {
                            parts.push(view! { <span class="gap"></span> }.into_any());
                        }
                        parts.push(c.into_any());
                    }
                    view! { <div class="shelf">{parts}</div> }
                }).collect_view();
                view! { {head} <div class="wood">{shelves}</div> }
            }}
        </div>
    }
}

fn picker_hint(s: &State) -> String {
    let who = seat_name(s, s.turn);
    match s.mode {
        Mode::Gosterge => "Gösterge taşına dokun".into(),
        Mode::Hand => format!("Taşlarına dokun · {} / {}", s.hand.len(), s.expected_hand()),
        Mode::PickDraw => "Çektiğin taşa dokun".into(),
        Mode::MeldEntry => "Perin taşlarına sırayla dokun".into(),
        Mode::PairEntry { okey: true, .. } => "Okeyle çift olacak taşa dokun".into(),
        Mode::PairEntry { .. } => "Çiftin taşına dokun".into(),
        _ => format!("{who} hangi taşı attı? Dokun"),
    }
}

#[component]
fn Picker() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    view! {
        <div class="picker">
            {move || {
                let s = st.get();
                let okey = okey_tile(&s);
                let seen = s.seen();
                let cell = |t: Tile| {
                    let on = s.key_enabled(t);
                    let left = 2u8.saturating_sub(seen[t as usize]);
                    let chosen = s.buffer.iter().filter(|&&b| b == t).count();
                    view! {
                        <button class="pk" class:one={on && left == 1} class:chosen={chosen > 0} disabled=!on on:click=move |_| ctx.act(|s| s.key(t))>
                            {tile_view(t, okey)}
                        </button>
                    }
                };
                let rows = (0..4u8).map(|c| {
                    let tiles = (1..=13u8).map(|n| cell(id(c, n))).collect_view();
                    let extra = if c == 0 { cell(JOKER).into_any() } else { view! { <span></span> }.into_any() };
                    view! { {tiles} {extra} }
                }).collect_view();
                view! {
                    <div class="picker-head">{picker_hint(&s)}</div>
                    <div class="grid">{rows}</div>
                }
            }}
        </div>
    }
}

#[component]
fn Message() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    view! {
        <Show when=move || ctx.msg.with(|m| m.is_some())>
            <div class="toast" on:click=move |_| ctx.msg.set(None)>{move || ctx.msg.get().unwrap_or_default()}</div>
        </Show>
    }
}

#[component]
fn Panel() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    move || {
        let s = st.get();
        let me = s.turn == 0;
        match (s.stage, s.mode) {
            (Stage::Setup, _) => setup_panel(ctx, &s).into_any(),
            (Stage::Over, _) => over_panel(ctx, &s).into_any(),
            (_, Mode::MeldEntry) => meld_entry(ctx, &s).into_any(),
            (_, Mode::Manual) if me => manual(ctx, &s).into_any(),
            (_, Mode::MeldMenu(i)) => meld_menu(ctx, &s, i).into_any(),
            (_, Mode::PairEntry { owner, okey }) => pair_entry(ctx, &s, owner, okey).into_any(),
            (_, Mode::PickDraw) => view! {
                <h2>"Hangi taşı çektin?"</h2>
                <p class="sub">"Çektiğin taşa dokun."</p>
                <span class="grow"></span>
                <button class="btn light" on:click=move |_| ctx.mode(Mode::Idle)>"Vazgeç"</button>
            }
            .into_any(),
            _ if me && s.may_act() && !s.opening_complete() => manual(ctx, &s).into_any(),
            _ if me && s.may_act() => view! { <Plan/> }.into_any(),
            _ if me => view! { <MyDraw/> }.into_any(),
            _ if s.may_act() => opp_act(ctx, &s).into_any(),
            _ => opp_draw(ctx, &s).into_any(),
        }
    }
}

fn setup_panel(ctx: Ctx, s: &State) -> impl IntoView + use<> {
    let step = |n: u8, now: u8, text: AnyView, back: Option<Mode>| {
        let cls = if n < now {
            "dot done"
        } else if n == now {
            "dot now"
        } else {
            "dot"
        };
        view! {
            <button class="step" class:clickable=back.is_some() disabled=back.is_none() on:click=move |_| if let Some(m) = back { ctx.mode(m) }>
                <span class=cls>{if n < now { "✓".to_string() } else { n.to_string() }}</span>
                <span>{text}</span>
            </button>
        }
    };
    let now = match s.mode {
        Mode::Gosterge => 1,
        Mode::Starter => 2,
        _ => 3,
    };
    let gtext = match s.gosterge {
        Some(g) => view! { "Gösterge " {tile_view(g, None)} }.into_any(),
        None => view! { "Gösterge" }.into_any(),
    };
    let stext = if now > 2 {
        view! { {format!("Başlayan: {} ({} taş)", SEAT_NAMES[s.starter as usize], s.expected_hand())} }.into_any()
    } else {
        view! { "Kim başlıyor" }.into_any()
    };
    let htext = view! { {format!("Taşların · {} / {}", s.hand.len(), s.expected_hand())}  }.into_any();
    let steps = view! {
        <div class="steps">
            {step(1, now, gtext, (now > 1).then_some(Mode::Gosterge))}
            {step(2, now, stext, (now > 2).then_some(Mode::Starter))}
            {step(3, now, htext, None)}
        </div>
    };
    match s.mode {
        Mode::Gosterge => view! {
            <h2>"Gösterge hangisi?"</h2>
            <p class="sub">"Ortada açık duran taşa dokun."</p>
            {steps}
        }
        .into_any(),
        Mode::Starter => view! {
            <h2>"Kim başlıyor?"</h2>
            {steps}
            <div class="choices">
                {(0..4u8).map(|p| view! {
                    <button class="btn light big" on:click=move |_| ctx.act(|s| { s.set_starter(p); Ok(()) })>{SEAT_NAMES[p as usize]}</button>
                }).collect_view()}
            </div>
        }
        .into_any(),
        _ => {
            let need = s.expected_hand();
            let have = s.hand.len();
            let full = have == need;
            view! {
                <h2>"Taşlarını gir"</h2>
                <p class="sub">"Taşlarına dokun. Yanlış girdiğin taşa yeşil alanda dokunursan silinir."</p>
                {steps}
                <div class="progress"><i style=format!("width:{}%", have * 100 / need.max(1))></i></div>
                <span class="grow"></span>
                <p class="muted">{match_line(s)}</p>
                <div class="actions">
                    <button class="btn big" class:go=full class:light=!full disabled=!full on:click=move |_| ctx.act(|s| s.start())>
                        {if full { "Başla".to_string() } else { format!("Başla ({} taş kaldı)", need - have) }}
                    </button>
                </div>
            }
            .into_any()
        }
    }
}

fn standing(s: &State) -> Option<String> {
    if s.history.is_empty() && s.totals == [0; 4] {
        return None;
    }
    let lead = s.lead();
    Some(if s.rules.partners {
        match lead[0] {
            x if x > 0 => format!("takım olarak {x} puan öndeyiz"),
            x if x < 0 => format!("takım olarak {} puan gerideyiz", -x),
            _ => "takımlar eşit".into(),
        }
    } else {
        let worst = *lead.iter().min().unwrap();
        if worst > 0 {
            format!("en yakın rakipten {worst} puan öndesin")
        } else if worst < 0 {
            format!("lidere {} puan geridesin", -worst)
        } else {
            "liderle eşitsin".into()
        }
    })
}

fn standing_short(s: &State) -> String {
    if s.history.is_empty() && s.totals == [0; 4] {
        return String::new();
    }
    let lead = s.lead();
    let x = if s.rules.partners { lead[0] } else { *lead.iter().min().unwrap() };
    if x >= 0 { format!(" · +{x}") } else { format!(" · −{}", -x) }
}

fn match_line(s: &State) -> String {
    let left = s.hands_total.saturating_sub(s.hand_no);
    let when = if left == 0 {
        "son el".to_string()
    } else {
        format!("{left} el kaldı")
    };
    match standing(s) {
        Some(x) => format!("{}. el / {} · {when} · {x}", s.hand_no, s.hands_total),
        None => format!("{}. el / {} · {when}", s.hand_no, s.hands_total),
    }
}

fn over_panel(ctx: Ctx, s: &State) -> impl IntoView + use<> {
    let next = format!("Yeni el ({} / {})", (s.hand_no + 1).min(s.hands_total), s.hands_total);
    let scores = s.hand_scores();
    let mult_note = s.finisher.map(|f| {
        let fin = seat_name(s, f);
        view! {
            <div class="mults">
                <span class="muted">{format!("{fin} nasıl bitti?")}</span>
                <label class="opt"><input type="checkbox" prop:checked=s.fin_okey
                    on:change=move |ev| { let v = event_target_checked(&ev); ctx.act(|s| { s.fin_okey = v; Ok(()) }) }/>"Okey atarak"</label>
                <label class="opt"><input type="checkbox" prop:checked=s.fin_elden
                    on:change=move |ev| { let v = event_target_checked(&ev); ctx.act(|s| { s.fin_elden = v; Ok(()) }) }/>"Elden"</label>
            </div>
        }
    });
    let rows = (0..4u8).map(|p| {
        let pv = &s.players[p as usize];
        let finished = s.finisher.is_some_and(|f| f == p || (s.rules.partners && (f + 2) % 4 == p));
        let score = scores.map(|x| x[p as usize].to_string()).unwrap_or_else(|| "?".into());
        let pen = s.penalties[p as usize];
        let what = if finished {
            view! { <span>{if s.finisher == Some(p) { "bitirdi" } else { "eşi bitirdi" }}</span> }.into_any()
        } else if p == 0 {
            view! { <span>{match pv.opened { Opened::No => "açmadın", Opened::Melds => "elinde kalan", Opened::Pairs => "elinde kalan ×2" }}</span> }.into_any()
        } else if pv.opened == Opened::No {
            let n = s.okeys_left[p as usize];
            view! {
                <span>"açmadı"</span>
                <button class="mini" on:click=move |_| ctx.act(|s| { s.okeys_left[p as usize] = (n + 1) % 3; Ok(()) })>
                    {if n == 0 { "+ okey".to_string() } else { format!("okey ×{n}") }}
                </button>
            }.into_any()
        } else {
            let pairs = pv.opened == Opened::Pairs;
            let val = s.left[p as usize].map(|v| v.to_string()).unwrap_or_default();
            view! {
                <input type="number" inputmode="numeric" placeholder="elinde kalan" prop:value=val
                    on:change=move |ev| {
                        let v = event_target_value(&ev).trim().parse::<i32>().ok();
                        ctx.act(|s| { s.left[p as usize] = v; Ok(()) })
                    }/>
                {pairs.then_some(" ×2")}
            }.into_any()
        };
        view! {
            <div class="score-row">
                <b>{seat_name(s, p)}</b>
                <span class="what">{what}</span>
                <span class="pen">{(pen != 0).then(|| format!("ceza {pen:+}"))}</span>
                <b class="num">{score}</b>
            </div>
        }
    }).collect_view();
    let after = scores.map(|sc| {
        let mut t = s.totals;
        for (x, y) in t.iter_mut().zip(sc) {
            *x += y;
        }
        let names: Vec<String> = (0..4u8).map(|p| format!("{} {}", seat_name(s, p), t[p as usize])).collect();
        view! { <p class="muted">{format!("Maç toplamı bu elden sonra: {}", names.join(" · "))}</p> }
    });
    let ready = scores.is_some();
    view! {
        <h2>{s.result.clone().unwrap_or_default()}</h2>
        <p class="sub">{if ready { "Puanlar hazır." } else { "Açan rakiplerin elinde kalan puanı yaz." }}</p>
        {mult_note}
        <div class="score-table">{rows}</div>
        {after}
        <span class="grow"></span>
        <div class="actions">
            <button class="btn go big" disabled=!ready on:click=move |_| ctx.act(|s| { *s = s.new_hand(); Ok(()) })>{next.clone()}</button>
            {(!ready).then(|| view! { <button class="btn light" on:click=move |_| ctx.act(|s| { *s = s.new_hand(); Ok(()) })>"Puan yazmadan geç"</button> })}
        </div>
    }
}

fn opp_draw(ctx: Ctx, s: &State) -> impl IntoView + use<> {
    let who = seat_name(s, s.turn);
    let side = s.side_tile();
    let okey = okey_tile(s);
    let deck = s.deck_count > 0;
    let from = seat_name(s, State::prev(s.turn));
    view! {
        <h2>{format!("{who} taşı nereden aldı?")}</h2>
        <div class="stack">
            {deck.then(|| view! { <button class="btn go big" on:click=move |_| ctx.act(|s| s.opp_draw())>"Desteden çekti"</button> })}
            {side.map(|t| view! {
                <button class="btn light big" on:click=move |_| ctx.act(|s| s.take_side())>"Yandan aldı " {tile_view(t, okey)}</button>
            })}
        </div>
        {side.map(|t| view! { <p class="muted">{format!("Yandaki taş {from} oyuncusunun attığı {}.", name(t))}</p> })}
        <span class="grow"></span>
        {(!deck).then(|| view! { <button class="btn danger" on:click=move |_| ctx.act(|s| { s.deck_out(); Ok(()) })>"Deste bitti, el kapandı"</button> })}
    }
}

fn opp_act(ctx: Ctx, s: &State) -> impl IntoView + use<> {
    let who = seat_name(s, s.turn);
    let okey = okey_tile(s);
    let pv = s.cur();
    let must = s.taken.map(|t| view! {
        <div class="warn">"Yandan aldığı " {tile_view(t, okey)} " taşını bu tur kullanmak zorunda: önce indirdiği peri gir ya da işlediği pere dokun."</div>
    });
    let opened_now = s.opened_this_turn.then(|| {
        let what = if pv.opened == Opened::Pairs {
            format!("{} çift", pv.open_value)
        } else {
            s.points(pv.open_value)
        };
        view! { <div class="good">{format!("Bu tur açtı: {what}")}</div> }
    });
    let can_tap = s.can_lay_off() && (0..s.table.melds.len()).any(|i| s.meld_tappable(i));
    let elden = pv.opened == Opened::No && !s.opened_this_turn;
    let short = (!s.opening_complete()).then(|| {
        view! {
            <div class="warn">{s.opening_progress()} " · açılış eksik. Kalan perleri gir; yanlışsa geri al."</div>
        }
    });
    view! {
        <h2>{format!("{who} ne yaptı?")}</h2>
        <p class="sub">"Attığı taşa dokun."</p>
        {must}
        {opened_now}
        {short}
        <div class="choices">
            <button class="btn light" on:click=move |_| ctx.mode(Mode::MeldEntry)>"Per / çift indirdi"</button>
            {elden.then(|| view! {
                <button class="btn light danger" on:click=move |_| ctx.act(|s| { let p = s.turn; s.finish(p, None); s.fin_elden = true; Ok(()) })>"Elden bitti"</button>
            })}
        </div>
        {(!elden).then(|| view! { <p class="muted">"Bitirdiyse: indirdiği ve işlediği taşları gir; son taşını atınca el kendiliğinden biter."</p> })}
        {can_tap.then(|| view! { <p class="muted">"İşlediyse ya da okey aldıysa masada o pere dokun (sarı çerçeveli)."</p> })}
    }
}

fn meld_entry(ctx: Ctx, s: &State) -> impl IntoView + use<> {
    let who = seat_name(s, s.turn);
    let okey = okey_tile(s);
    let problem = s.meld_problem();
    let ok = problem.is_none();
    let status = match (&problem, s.buffer_shape()) {
        (None, Some(sh)) if sh.is_pair() => "Çift ✓".to_string(),
        (None, Some(sh)) => format!("{} ✓ · {}", shape_label(&sh), s.points(sh.open_value(&s.rules))),
        (Some(p), _) => p.clone(),
        _ => String::new(),
    };
    let total = s.opening_progress();
    let total_class = if s.opening_complete() { "good" } else { "warn" };
    let buffer = s.buffer.clone();
    view! {
        <h2>{format!("{who} hangi taşları indirdi?")}</h2>
        <p class="sub">"Bir perin taşlarına sırayla dokun, sonra ekle. Birden fazla per varsa tek tek."</p>
        <div class="buffer">
            {buffer.into_iter().enumerate().map(|(i, t)| view! {
                <button class="tbtn" title="Çıkar" on:click=move |_| ctx.act(|s| { s.buffer.remove(i); Ok(()) })>{tile_view(t, okey)}</button>
            }).collect_view()}
        </div>
        <div class=if ok { "good" } else { "muted" }>{status}</div>
        {total.map(|t| view! { <div class=total_class>{t}</div> })}
        <span class="grow"></span>
        <div class="choices">
            <button class="btn go" disabled=!ok on:click=move |_| ctx.act(|s| s.lay_meld())>"Bu peri ekle"</button>
            <button class="btn light" on:click=move |_| ctx.mode(Mode::Idle)>"Tamam, bitti"</button>
        </div>
    }
}

fn option_label(o: &LayOption) -> String {
    match (o.tile == JOKER, o.side) {
        (false, Side::Group) => "gruba ekle".to_string(),
        (false, side) => side_text(side).to_string(),
        (true, Side::Group) => "okeyi gruba ekle".to_string(),
        (true, side) => format!("{} ({} yerine)", side_text(side), name(o.stands_for)),
    }
}

fn okey_fine(losers: &[u8], s: &State) -> String {
    let who: Vec<&str> = losers.iter().map(|&p| SEAT_NAMES[p as usize]).collect();
    format!("okeyi koyana {} ceza ({})", s.rules.okey_lost_penalty, who.join(", "))
}

fn meld_menu(ctx: Ctx, s: &State, idx: usize) -> impl IntoView + use<> {
    let okey = okey_tile(s);
    let me = s.turn == 0;
    let who = seat_name(s, s.turn);
    let Some(m) = s.table.melds.get(idx).cloned() else {
        return view! { <button class="btn light" on:click=move |_| ctx.mode(Mode::Idle)>"Geri"</button> }.into_any();
    };
    let lays = s.lay_options(idx);
    let swaps: Vec<JokerSlot> = s.swap_options(idx);
    let pair = m.shape.is_pair() && s.can_add_pair(m.owner);
    let owner = m.owner;
    let nothing = lays.is_empty() && swaps.is_empty() && !pair;
    view! {
        <h2>{if me { "Bu pere ne yapıyorsun?".to_string() } else { format!("{who} bu pere ne yaptı?") }}</h2>
        <div class="buffer">{tiles_view(&m.shape.tiles(), okey)}</div>
        <div class="stack">
            {lays.into_iter().map(|o| view! {
                <button class="btn light opt" on:click=move |_| ctx.act(|s| s.lay_option(idx, o))>
                    {tile_view(o.tile, okey)} <span>{option_label(&o)}</span>
                </button>
            }).collect_view()}
            {swaps.into_iter().map(|slot| {
                let give = slot.tiles.clone();
                let n = slot.jokers;
                let losers = s.okey_losers(&slot);
                let pen = (!losers.is_empty()).then(|| format!(" · {}", okey_fine(&losers, s)));
                view! {
                    <button class="btn light opt" on:click=move |_| ctx.act(|s| s.swap(&slot))>
                        {tiles_view(&give, okey)}
                        <span>{format!("koyup {} al", if n > 1 { "2 okeyi" } else { "okeyi" })}{pen}</span>
                    </button>
                }
            }).collect_view()}
            {pair.then(|| view! {
                <button class="btn light opt" on:click=move |_| ctx.mode(Mode::PairEntry { owner, okey: false })>"Buraya çift işle"</button>
            })}
        </div>
        {nothing.then(|| view! { <p class="muted">"Bu pere şu an eklenebilecek bir şey yok (uç başına turda en fazla 2 taş)."</p> })}
        <span class="grow"></span>
        <button class="btn light" on:click=move |_| ctx.mode(if me { Mode::Manual } else { Mode::Idle })>"Tamam"</button>
    }
    .into_any()
}

fn pair_entry(ctx: Ctx, s: &State, owner: u8, okey: bool) -> impl IntoView + use<> {
    let me = s.turn == 0;
    let room = s.table.pair_room(owner, &s.rules);
    view! {
        <h2>{format!("{} çiftlerine hangi çift?", SEAT_OWNER[owner as usize])}</h2>
        <p class="sub">{format!("Taşa dokun; çift olarak eklenir. Bu tur {room} çift daha işlenebilir.")}</p>
        <div class="choices">
            <button class="btn light" class:on=okey on:click=move |_| ctx.mode(Mode::PairEntry { owner, okey: !okey })>
                {if okey { "Okeyli çift: açık" } else { "Okeyli çift: kapalı" }}
            </button>
            <button class="btn light" on:click=move |_| ctx.mode(if me { Mode::Manual } else { Mode::Idle })>"Tamam"</button>
        </div>
    }
}

fn manual(ctx: Ctx, s: &State) -> impl IntoView + use<> {
    let okey = okey_tile(s);
    let n = s.buffer.len();
    let problem = s.meld_problem();
    let can_meld = problem.is_none() && n >= 2;
    let status = match (n, &problem, s.buffer_shape()) {
        (0, _, _) => "İstakadan taşlara dokun.".to_string(),
        (1, _, _) => "Tek taş seçili: atabilirsin. İşlemek için masadaki pere dokun.".to_string(),
        (_, None, Some(sh)) if sh.is_pair() => "Çift ✓".to_string(),
        (_, None, Some(sh)) => format!("{} ✓ · {}", shape_label(&sh), s.points(sh.open_value(&s.rules))),
        (_, Some(p), _) => p.clone(),
        _ => String::new(),
    };
    let complete = s.opening_complete();
    let single = (n == 1 && complete).then(|| s.buffer[0]);
    let buffer = s.buffer.clone();
    let short = (!complete).then(|| view! {
        <div class="warn">{s.opening_progress()} " · açılış tamamlanmadan işleyemez, okey alamaz, taş atamazsın. Eksik perleri indir ya da geri al."</div>
    });
    view! {
        <h2>"Kendi hamlen"</h2>
        {short}
        <div class="buffer">{tiles_view(&buffer, okey)}</div>
        <div class=if can_meld { "good" } else { "muted" }>{status}</div>
        <span class="grow"></span>
        <div class="stack">
            {can_meld.then(|| view! { <button class="btn go" on:click=move |_| ctx.act(|s| s.lay_meld())>"İndir"</button> })}
            {single.map(|t| view! {
                <button class="btn go" on:click=move |_| ctx.act(|s| { s.buffer.clear(); s.discard(0, t) })>"Bu taşı at " {tile_view(t, okey)}</button>
            })}
            {complete.then(|| view! { <button class="btn light" on:click=move |_| ctx.mode(Mode::Idle)>"Öneriye dön"</button> })}
        </div>
    }
}

fn find(r: &Report, d: Decision) -> Option<&CandReport> {
    r.candidates.iter().find(|c| c.decision == d)
}

fn advantage(c: &CandReport) -> String {
    let lead = -c.gap;
    if lead >= 0.0 {
        format!("rakiplerden {lead:.0} puan önde")
    } else {
        format!("rakiplerden {:.0} puan geride", -lead)
    }
}

fn last_if_all_draw(deck: u8, i_draw: bool) -> Option<u8> {
    if deck == 0 {
        return None;
    }
    let first = if i_draw { 0 } else { 1 };
    Some(((first + deck as u32 - 1) % 4) as u8)
}

fn last_line(s: &State, c: &CandReport) -> String {
    let p = c.p_last_draw;
    if s.rules.partners {
        format!(
            "biz {} (ben {}, eşim {}) · onlar {}",
            pct(p[0] + p[2]),
            pct(p[0]),
            pct(p[2]),
            pct(p[1] + p[3])
        )
    } else {
        let mut order: Vec<usize> = (0..4).collect();
        order.sort_by(|&a, &b| p[b].total_cmp(&p[a]));
        order
            .iter()
            .filter(|&&i| p[i] >= 0.005)
            .map(|&i| format!("{} {}", seat_name(s, i as u8), pct(p[i])))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

#[component]
fn MyDraw() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    let can_take = Memo::new(move |_| st.with(|s| s.can_take()));
    move || {
        let s = st.get();
        let okey = okey_tile(&s);
        let side = s.side_tile();
        let take_ok = can_take.get();
        let deck = s.deck_count > 0;
        let report = ctx.analysis.get().and_then(|r| r.report);
        let draw = report.as_ref().and_then(|r| find(r, Decision::Draw).cloned());
        let take = report.as_ref().and_then(|r| find(r, Decision::Take).cloned());
        let best_take = matches!((&draw, &take), (Some(d), Some(t)) if t.mean < d.mean) || (!deck && take_ok);
        let title = match (&draw, &take) {
            (Some(_), Some(_)) if best_take => view! { "Yandan al " {side.map(|t| tile_view(t, okey))} }.into_any(),
            (Some(_), Some(_)) => view! { "Desteden çek" }.into_any(),
            (None, None) if take_ok => view! { "Hesaplanıyor…" }.into_any(),
            _ => view! { "Sıra sende: taş çek" }.into_any(),
        };
        let sub = match (&draw, &take) {
            (Some(d), Some(t)) => match d.p_win.zip(t.p_win) {
                Some((dw, tw)) => format!("Maçı kazanma {} · diğeri {}", chance(dw.max(tw)), chance(dw.min(tw))),
                None => format!("Diğerinden {:.1} puan daha iyi", (d.mean - t.mean).abs()),
            },
            (Some(_), None) if side.is_some() && !take_ok => "Yandaki taşı bu tur kullanamazsın, alınamaz.".to_string(),
            _ => String::new(),
        };
        let bars = draw.as_ref().zip(take.as_ref()).map(|(d, t)| {
            let best = d.mean.min(t.mean);
            let width = |c: &CandReport| (100.0 - (c.mean - best) * 2.0).clamp(8.0, 100.0);
            view! {
                <div class="bars">
                    <div class="bar-row" class:best=best_take><span>"Yandan al " {side.map(|x| tile_view(x, okey))}</span><span class="bar"><i style=format!("width:{:.0}%", width(t))></i></span></div>
                    <div class="bar-row" class:best=!best_take><span>"Desteden çek"</span><span class="bar"><i style=format!("width:{:.0}%", width(d))></i></span></div>
                </div>
            }
        });
        let rule = deck.then(|| {
            let take_last = last_if_all_draw(s.deck_count, false).map(|p| seat_name(&s, p));
            let draw_last = last_if_all_draw(s.deck_count, true).map(|p| seat_name(&s, p));
            let row = |label: &'static str, last: Option<String>, c: Option<&CandReport>| {
                let sim = c.filter(|c| c.whole >= ENOUGH_ENDINGS).map(|c| {
                    let reach: f64 = c.p_last_draw.iter().sum();
                    format!(
                        "Simülasyonda: {} · {} ihtimalle el ondan önce biter",
                        last_line(&s, c),
                        pct(1.0 - reach)
                    )
                });
                view! {
                    <div class="lt-row">
                        <span>{label}": herkes desteden çekerse son taş "<b>{last.unwrap_or_default()}</b></span>
                        {sim.map(|x| view! { <span class="muted">{x}</span> })}
                    </div>
                }
            };
            view! {
                <div class="lasttile">
                    <b>"Destenin son taşı kime gider?"</b>
                    {take_ok.then(|| row("Yandan alırsan", take_last.clone(), take.as_ref()))}
                    {row("Desteden çekersen", draw_last.clone(), draw.as_ref())}
                </div>
            }
        });
        view! {
            <h2>{title}</h2>
            <p class="sub">{sub}</p>
            {bars}
            {rule}
            <span class="grow"></span>
            <div class="actions"><div class="choices">
                {side.filter(|_| take_ok).map(|t| view! {
                    <button class="btn" class:go=best_take class:light=!best_take on:click=move |_| ctx.act(|s| s.apply(Decision::Take))>"Yandan aldım " {tile_view(t, okey)}</button>
                })}
                {deck.then(|| view! {
                    <button class="btn" class:go=!best_take class:light=best_take on:click=move |_| ctx.act(|s| s.apply(Decision::Draw))>"Desteden çektim"</button>
                })}
            </div>
            {(!deck && !take_ok).then(|| view! {
                <button class="btn danger" on:click=move |_| ctx.act(|s| { s.deck_out(); Ok(()) })>"Deste bitti, el kapandı"</button>
            })}
            </div>
        }
    }
}

fn plan_title(steps: &[Step], s: &State) -> String {
    let mut parts = Vec::new();
    let melds: Vec<&Shape> = steps
        .iter()
        .filter_map(|x| {
            if let Step::Meld { shape, owner: 0 } = x {
                Some(shape)
            } else {
                None
            }
        })
        .collect();
    let unopened = s.players[0].opened == Opened::No;
    if !melds.is_empty() {
        let pairs = melds.iter().all(|m| m.is_pair());
        if unopened && pairs {
            parts.push(format!("Çiftle aç ({})", melds.len()));
        } else if unopened {
            let total: i32 = melds.iter().map(|m| m.open_value(&s.rules)).sum();
            parts.push(format!("Aç ({})", s.total(total)));
        } else {
            parts.push(format!("{} per indir", melds.len()));
        }
    } else if unopened {
        parts.push("Açma".into());
    }
    let lays: Vec<Tile> = steps
        .iter()
        .filter_map(|x| if let Step::Layoff { tile, .. } = x { Some(*tile) } else { None })
        .collect();
    match lays.len() {
        0 => {}
        1 => parts.push(format!(
            "{} işle",
            if lays[0] == JOKER { "okeyi".to_string() } else { name(lays[0]) }
        )),
        n => parts.push(format!("{n} taş işle")),
    }
    if steps.iter().any(|x| matches!(x, Step::Swap { .. })) {
        parts.push("okey al".into());
    }
    if steps.iter().any(|x| matches!(x, Step::Meld { owner, .. } if *owner != 0)) {
        parts.push("çift işle".into());
    }
    for x in steps {
        if let Step::Discard { tile, finish } = x {
            parts.push(if *finish {
                format!("{} atıp bitir", name(*tile))
            } else {
                format!("{} at", name(*tile))
            });
        }
    }
    parts.join(" · ")
}

fn step_view(step: &Step, n: usize, okey: Option<Tile>, s: &State) -> impl IntoView + use<> {
    let dot = |label: String, style: String| view! { <span class="dot num" style=style>{label}</span> };
    let color_of = |n: usize| format!("background:{}", GROUP_COLORS[(n - 1) % GROUP_COLORS.len()]);
    match step.clone() {
        Step::Meld { shape, owner } => {
            let tiles = shape.tiles();
            let head = if shape.is_pair() {
                if owner == 0 { "çift".to_string() } else { format!("{} çiftlerine", SEAT_OWNER[owner as usize]) }
            } else {
                format!("{} · {}", shape_label(&shape).to_lowercase(), s.total(shape.open_value(&s.rules)))
            };
            let note = okey_note(&shape);
            view! {
                <li>{dot(n.to_string(), color_of(n))}<span class="row">{tiles_view(&tiles, okey)}</span><span class="what">{head}{note.map(|x| view! { <small>{x}</small> })}</span></li>
            }
            .into_any()
        }
        Step::Layoff { tile, owner, onto, side, stands_for } => {
            let whose = if owner == 0 { "kendi".to_string() } else { SEAT_OWNER[owner as usize].to_string() };
            let place = match side {
                Side::Group => "perine".to_string(),
                side => format!("perinin {}", side_text(side)),
            };
            let note = (tile == JOKER && stands_for != JOKER).then(|| format!("okey = {}", name(stands_for)));
            view! {
                <li>{dot(n.to_string(), "background:#fff;color:#111;box-shadow:inset 0 0 0 1.5px #111".into())}
                    <span class="row">{tile_view(tile, okey)}</span>
                    <span class="what">{format!("{whose} {place}")}<span class="row mini">{tiles_view(&onto, okey)}</span>{note.map(|x| view! { <small>{x}</small> })}</span>
                </li>
            }
            .into_any()
        }
        Step::Swap {
            owner,
            onto,
            give,
            jokers,
            losers,
        } => {
            let pen = (!losers.is_empty()).then(|| format!(" · {}", okey_fine(&losers, s)));
            view! {
                <li>{dot(n.to_string(), "background:#c084fc".into())}
                    <span class="row">{tiles_view(&give, okey)}</span>
                    <span class="what">{format!("koy, {} perinden {} okey al{}", SEAT_OWNER[owner as usize], jokers, pen.unwrap_or_default())}<span class="row mini">{tiles_view(&onto, okey)}</span></span>
                </li>
            }
            .into_any()
        }
        Step::Discard { tile, finish } => view! {
            <li>{dot("AT".into(), "background:#ef4444".into())}<span class="row">{tile_view(tile, okey)}</span><span class="what">{if finish { "at ve bitir" } else { "at" }}</span></li>
        }
        .into_any(),
    }
}

#[component]
fn Plan() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    move || {
        let s = st.get();
        let okey = okey_tile(&s);
        let resp = ctx.analysis.get();
        if let Some(Resp { error: Some(e), .. }) = &resp {
            return view! { <div class="warn">{e.clone()}</div> }.into_any();
        }
        let Some((d, mv)) = ctx.chosen.get() else {
            return view! {
                <h2>"Sıra sende"</h2>
                <p class="sub">"En iyi hamle hesaplanıyor…"</p>
                <span class="grow"></span>
                <button class="btn light" on:click=move |_| ctx.mode(Mode::Manual)>"Kendim gireceğim"</button>
            }
            .into_any();
        };
        let report = resp.as_ref().and_then(|r| r.report.clone());
        let cand = report.as_ref().and_then(|r| find(r, d).cloned());
        let is_best = ctx.best.get().is_some_and(|(b, _)| b == d);
        let screening = report.as_ref().is_some_and(|r| r.screening);
        let done = resp.as_ref().is_some_and(|r| r.done);
        let settled = done || report.as_ref().is_some_and(|r| r.settled);
        let close = report.as_ref().is_some_and(|r| r.close);
        let doubt = report
            .as_ref()
            .filter(|r| r.iterations >= 100 && r.doubt.is_finite())
            .map(|r| r.doubt);
        let match_mode = cand.as_ref().is_some_and(|c| c.p_win.is_some());
        let state_note = if screening {
            ("Bütün hamleler taranıyor, öneri değişebilir".to_string(), "pending")
        } else if !settled {
            match doubt {
                Some(d) if match_mode => (
                    format!("Hesap sürüyor · başka bir hamle maç şansını en fazla %{:.1} artırabilir", d / 10.0),
                    "pending",
                ),
                Some(d) => (
                    format!("Hesap sürüyor · başka bir hamle en fazla {:.0} puan daha iyi olabilir", d.ceil()),
                    "pending",
                ),
                None => ("Hesap sürüyor, öneri değişebilir".to_string(), "pending"),
            }
        } else if close && is_best {
            ("Kesinleşti · ilk iki seçenek neredeyse eşit".to_string(), "done")
        } else {
            ("Kesinleşti".to_string(), "done")
        };
        let steps = ctx.steps.get().unwrap_or_default();
        let title = match &mv {
            Some(m) => move_short(m, &s),
            None => plan_title(&steps, &s),
        };
        let stats = cand.as_ref().map(|c| {
            let aim = match c.p_win {
                Some(w) => format!("maçı kazanma {} · bu elde {}", chance(w), advantage(c)),
                None => advantage(c),
            };
            let finish = if c.whole >= ENOUGH_ENDINGS {
                format!(" · bitirme {}", pct(c.p_finish))
            } else {
                String::new()
            };
            format!("{aim}{finish} · sağdaki attığını alır {}", pct(c.p_next_takes))
        });
        let model_on = s.model && s.view().is_some_and(|v| model_fits(&v, v.rules.partners));
        let status = resp.as_ref().zip(report.as_ref()).map(|(r, rep)| {
            let cut = if rep.truncated {
                " · seçenek çok, bir kısmı incelenemedi"
            } else {
                ""
            };
            let ai = if rep.deep {
                " · sonraki turun da hesapta"
            } else if model_on {
                " · sonraki turlarını yapay zekâ oynuyor"
            } else {
                ""
            };
            format!("{} senaryo{}{cut}{ai}", rep.iterations, if r.done { "" } else { "…" })
        });
        let mut k = 0;
        let list = steps
            .iter()
            .map(|x| {
                if !matches!(x, Step::Discard { .. }) {
                    k += 1;
                }
                step_view(x, k, okey, &s)
            })
            .collect_view();
        let alts = report.clone().map(|r| alternatives(ctx, &r, &s));
        view! {
            <h2>{if is_best { title } else { format!("Seçtiğin: {title}") }}</h2>
            <p class="sub">{stats}</p>
            <ol class="plan">{list}</ol>
            <span class="grow"></span>
            <div class="actions">
                <div class=format!("state {}", state_note.1)>{state_note.0}" · "{status}</div>
                <button class="btn go big" on:click=move |_| ctx.act(|s| match &mv {
                    Some(m) => s.apply_move(m),
                    None => s.apply(d),
                })>"Yaptım"</button>
                <button class="btn light" on:click=move |_| ctx.mode(Mode::Manual)>"Başka bir şey yaptım"</button>
            </div>
            {alts}
        }
        .into_any()
    }
}

fn move_short(mv: &Move, s: &State) -> String {
    let unopened = s.players[0].opened == Opened::No;
    let (mut value, mut melds, mut pairs, mut gifts, mut swaps) = (0, 0, 0, 0, false);
    let mut laid: Vec<Tile> = Vec::new();
    let mut left = s.hand.len() as i32 - 1;
    for act in &mv.acts {
        match act {
            Act::Meld(sh) if sh.is_pair() => {
                pairs += 1;
                left -= 2;
            }
            Act::Meld(sh) => {
                melds += 1;
                value += sh.open_value(&s.rules);
                left -= sh.len() as i32;
            }
            Act::Layoff { tile, .. } => {
                laid.push(*tile);
                left -= 1;
            }
            Act::Swap(slot) => {
                swaps = true;
                left += slot.jokers as i32 - slot.tiles.len() as i32;
            }
            Act::Gift { .. } => {
                gifts += 1;
                left -= 2;
            }
        }
    }
    let mut parts = Vec::new();
    match (unopened, melds, pairs) {
        (true, 0, 0) => parts.push("Açma".to_string()),
        (true, 0, n) => parts.push(format!("{n} çiftle aç")),
        (true, _, _) => parts.push(format!("Aç {}", s.total(value))),
        (false, 0, 0) => {}
        (false, 0, n) => parts.push(format!("{n} çift indir")),
        (false, n, _) => parts.push(format!("{n} per indir")),
    }
    match laid[..] {
        [] => {}
        [t] => parts.push(format!("{} işle", if t == JOKER { "okeyi".to_string() } else { name(t) })),
        _ => parts.push(format!("{} taş işle", laid.len())),
    }
    if swaps {
        parts.push("okey al".into());
    }
    if gifts > 0 {
        parts.push(format!("{gifts} çift işle"));
    }
    let tile = if mv.discard == JOKER {
        "Okey".to_string()
    } else {
        name(mv.discard)
    };
    parts.push(if left == 0 {
        format!("{tile} atıp bitir")
    } else {
        format!("{tile} at")
    });
    parts.join(" · ")
}

fn alternatives(ctx: Ctx, report: &Report, s: &State) -> impl IntoView + use<> {
    let Some(top) = report.candidates.first().cloned() else {
        return ().into_any();
    };
    let picked = ctx.pick.get().map_or(top.decision, |(d, _)| d);
    let rows: Vec<CandReport> = report
        .candidates
        .iter()
        .filter(|c| c.mv.is_some() || !matches!(c.decision, Decision::Play(_)))
        .take(6)
        .cloned()
        .collect();
    if rows.len() < 2 {
        return ().into_any();
    }
    let mut seen: Vec<String> = Vec::new();
    let labels: Vec<String> = rows
        .iter()
        .map(|c| {
            let label = match &c.mv {
                Some(mv) => move_short(mv, s),
                None => c.label.clone(),
            };
            let again = seen.contains(&label);
            seen.push(label.clone());
            if again { format!("{label} (başka dizilişle)") } else { label }
        })
        .collect();
    view! {
        <div class="bars">
            <span class="muted">"Seçenekler · dokununca adımları görünür"</span>
            {rows.into_iter().zip(labels).map(|(c, label)| {
                let d = c.decision;
                let mv = c.mv.clone();
                let behind = c.mean - top.mean;
                let width = (100.0 - behind * 2.0).clamp(6.0, 100.0);
                let note = match (c.p_win, top.p_win) {
                    (Some(w), Some(_)) => chance(w),
                    _ if d == top.decision => "önerilen".to_string(),
                    _ if behind < 0.05 => "eşit".to_string(),
                    _ if c.alive => format!("{behind:.1} puan geride"),
                    _ => format!("{behind:.0} puan geride"),
                };
                view! {
                    <button class="bar-row alt" class:best={picked == d} on:click=move |_| ctx.pick.set(Some((d, mv.clone())))>
                        <span class="lbl">{label}</span>
                        <span class="bar"><i style=format!("width:{width:.0}%")></i></span>
                        <span class="note">{note}</span>
                    </button>
                }
            }).collect_view()}
        </div>
    }
    .into_any()
}

#[component]
fn MatchBox() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    let number = move |label: String, get: Box<dyn Fn(&State) -> i32 + Send + Sync>, set: fn(&mut State, i32)| {
        view! {
            <label class="opt num">
                <span>{label}</span>
                <input type="number" inputmode="numeric"
                    prop:value=move || st.with(|s| get(s).to_string())
                    on:change=move |ev| {
                        if let Ok(v) = event_target_value(&ev).trim().parse::<i32>() {
                            ctx.act(|s| { set(s, v); Ok(()) })
                        }
                    }
                />
            </label>
        }
    };
    view! {
        <div class="matchform">
            <p class="muted">{move || st.with(match_line)}</p>
            {number("Kaçıncı eldeyiz".into(), Box::new(|s| s.hand_no as i32), |s, v| s.hand_no = v.clamp(1, 99) as u8)}
            {number("Maç kaç el".into(), Box::new(|s| s.hands_total as i32), |s, v| s.hands_total = v.clamp(1, 99) as u8)}
            <details>
                <summary>"Puan tablosunu düzelt"</summary>
                {number("Ben".into(), Box::new(|s| s.totals[0]), |s, v| s.totals[0] = v)}
                {number("Sağ".into(), Box::new(|s| s.totals[1]), |s, v| s.totals[1] = v)}
                {number("Karşı".into(), Box::new(|s| s.totals[2]), |s, v| s.totals[2] = v)}
                {number("Sol".into(), Box::new(|s| s.totals[3]), |s, v| s.totals[3] = v)}
                <button class="btn light danger wide" on:click=move |_| ctx.act(|s| { *s = s.new_match(); Ok(()) })>"Yeni maç başlat (puanları sıfırla)"</button>
            </details>
            <p class="muted">"Maç boyunca farkı açmaya oynar. Son iki elde puan tablosuna bakıp maçı kazanmaya oynar: gerideyken risk alır, öndeyken garantiye oynar."</p>
        </div>
    }
}

#[component]
fn Info() -> impl IntoView {
    view! {
        <h3>"Atılan taşlar"</h3>
        <Discards/>
        <h3>"Elimin durumu"</h3>
        <HandInfo/>
        <h3>"Rakiplerin tahmini eli"</h3>
        <Reads/>
        <h3>"Görülmemiş taşlar"</h3>
        <Unseen/>
        <CopyState/>
    }
}

#[component]
fn CopyState() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let copied = RwSignal::new(false);
    view! {
        <button class="btn light wide" on:click=move |_| {
            if let (Some(w), Ok(j)) = (web_sys::window(), serde_json::to_string(&ctx.st.get_untracked())) {
                let _ = w.navigator().clipboard().write_text(&j);
                copied.set(true);
            }
        }>
            {move || if copied.get() { "Kopyalandı ✓ (hata bildirirken yapıştır)" } else { "Bu durumu kopyala (hata bildirimi için)" }}
        </button>
    }
}

#[component]
fn Discards() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    move || {
        let s = st.get();
        let okey = okey_tile(&s);
        (0..4u8)
            .map(|p| {
                let pv = &s.players[p as usize];
                let known = (!pv.known.is_empty()).then(|| view! { <span class="muted">" elinde: "</span> {tiles_view(&pv.known, okey)} });
                view! {
                    <div class="disc-row">
                        <span class="seatname">{seat_name(&s, p)}</span>
                        <span class="row wrap">{tiles_view(&pv.discards, okey)}</span>
                        {known}
                    </div>
                }
            })
            .collect_view()
    }
}

#[component]
fn HandInfo() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    move || {
        let okey = okey_tile(&st.get());
        ctx.hand.get().map(|r: HandReport| {
            let (opened, yan) = st.with(|s| (s.players[0].opened, s.yan));
            let total = move |v: i32| state::count(v, yan);
            let head = match opened {
                Opened::No => format!("Perle açış: {} / {} · Çift: {} / {}", total(r.open_value), total(r.meld_req), r.pairs, r.pair_req),
                Opened::Melds => "Perle açtın.".to_string(),
                Opened::Pairs => "Çiftle açtın.".to_string(),
            };
            view! {
                <div>{head}</div>
                <div class="row wrap">
                    {r.melds.iter().map(|m| view! { <span class="grp">{tiles_view(m, okey)}</span> }).collect_view()}
                </div>
                <p class="muted">"Taş × görülmemiş adet → gelirse açış değerin / çift sayın"</p>
                <div class="outs">
                    {r.outs.iter().take(16).map(|o| view! {
                        <span class="out">{tile_view(o.tile, okey)}<small>{format!("×{} → {} / {}ç", o.unseen, total(o.open_value), o.pairs)}</small></span>
                    }).collect_view()}
                </div>
            }
        })
    }
}

fn share(p: f32) -> String {
    pct(p as f64)
}

/// The smallest and largest pair counts once the 10% least likely at each end are left out.
fn pair_range(dist: &[f32]) -> (usize, usize) {
    let mut acc = 0.0;
    let lo = dist.iter().position(|&x| {
        acc += x;
        acc > 0.1
    });
    acc = 0.0;
    let hi = dist.iter().rposition(|&x| {
        acc += x;
        acc > 0.1
    });
    (lo.unwrap_or(0), hi.unwrap_or(0))
}

fn pairs_text(dist: &[f32]) -> String {
    let more = |k: usize| if k == MAX_PAIRS { "+" } else { "" };
    match pair_range(dist) {
        (a, b) if a == b => format!("{a}{} çift", more(a)),
        (a, b) => format!("{a}–{b}{} çift", more(b)),
    }
}

fn read_line(r: &Read, s: &State) -> String {
    match r.opened {
        Opened::No => {
            let mut parts = vec![format!("per {}–{}", s.total(r.per[0]), s.total(r.per[2]))];
            if s.rules.pairs_enabled {
                parts.push(pairs_text(&r.pairs));
            }
            parts.push(format!("açabilir {}", share(r.p_either)));
            parts.join(" · ")
        }
        _ => format!("elinde {}–{} puan", r.points[0], r.points[2]),
    }
}

#[component]
fn Reads() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    move || {
        let s = st.get();
        let reads = ctx.reads.get()?;
        let rows = OPPONENTS
            .iter()
            .filter_map(|&p| reads.iter().find(|r| r.seat == p).cloned())
            .map(|r| {
                let detail = match r.opened {
                    Opened::No => {
                        let bands = r
                            .per_bands
                            .iter()
                            .enumerate()
                            .filter(|&(_, &x)| x >= 0.03)
                            .map(|(i, &x)| {
                                let lo = i as i32 * BAND;
                                let reach = lo + BAND > r.meld_req;
                                view! {
                                    <div class="bar" class:reach=reach>
                                        <span class="bl">{format!("{}–{}", s.total(lo), s.total(lo + BAND - 1))}</span>
                                        <span class="bt"><span class="bf" style=format!("width:{:.0}%", x * 100.0)></span></span>
                                        <span class="bp">{share(x)}</span>
                                    </div>
                                }
                            })
                            .collect_view();
                        let pairs = s.rules.pairs_enabled.then(|| {
                            let cells = r
                                .pairs
                                .iter()
                                .enumerate()
                                .filter(|&(_, &x)| x >= 0.01)
                                .map(|(k, &x)| {
                                    let more = if k == MAX_PAIRS { "+" } else { "" };
                                    view! { <span class="pc" class:reach={k >= r.pair_req as usize}>{format!("{k}{more} çift {}", share(x))}</span> }
                                })
                                .collect_view();
                            view! { <div class="row wrap pcs">{cells}</div> }
                        });
                        let odds = if s.rules.pairs_enabled {
                            format!(
                                "Şu an {}+ per açabilir {} · {} çift açabilir {} · ikisinden biri {}",
                                s.total(r.meld_req),
                                share(r.p_melds),
                                r.pair_req,
                                share(r.p_pairs),
                                share(r.p_either)
                            )
                        } else {
                            format!("Şu an {}+ per açabilir {}", s.total(r.meld_req), share(r.p_melds))
                        };
                        view! {
                            <div class="muted">
                                {format!("En iyi per: büyük ihtimalle {} (%80 aralık {}–{})", s.total(r.per[1]), s.total(r.per[0]), s.total(r.per[2]))}
                            </div>
                            <div class="bars">{bands}</div>
                            {pairs}
                            <div>{odds}</div>
                        }
                        .into_any()
                    }
                    _ => view! {
                        <div>{format!("El şimdi bitse yazılacak: büyük ihtimalle {} (%80 aralık {}–{})", r.points[1], r.points[0], r.points[2])}</div>
                    }
                    .into_any(),
                };
                view! {
                    <div class="read">
                        <b>{seat_name(&s, r.seat)}</b>
                        {detail}
                    </div>
                }
            })
            .collect_view();
        Some(view! {
            <p class="muted">
                "Görülmemiş taşlar, rakiplerin attığı taşlara ve açmamış olmalarına uyacak biçimde 200 kez dağıtılıp sayılır. "
                "Elini tutup geç açan rakiplerde açabilme ihtimali olduğundan düşük, hazır olunca hemen açanlarda yüksek çıkar."
            </p>
            {rows}
        })
    }
}

#[component]
fn Unseen() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    move || {
        let s = st.get();
        let seen = s.seen();
        let okey = okey_tile(&s);
        let left = |t: Tile| 2u8.saturating_sub(seen[t as usize]);
        let total: u32 = (0..NIDS).map(|i| left(i as Tile) as u32).sum();
        let cell = |t: Tile| view! { <span class="ucell" class:none={left(t) == 0}>{tile_view(t, okey)}<sub>{left(t)}</sub></span> };
        view! {
            <p class="muted">{format!("Toplam {total} taş (rakiplerin eli + deste)")}</p>
            <div class="ugrid">
                {(0..4u8).map(|c| view! { {(1..=13u8).map(|n| cell(id(c, n))).collect_view()} {if c == 0 { cell(JOKER).into_any() } else { view! { <span></span> }.into_any() }} }).collect_view()}
            </div>
        }
    }
}
