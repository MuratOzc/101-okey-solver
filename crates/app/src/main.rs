mod replay;
mod settings;
mod state;

use leptos::prelude::*;
use okey_engine::analysis::{
    Analyzer, CandReport, Decision, Field, HandReport, Packed, Phase, Report, View, describe, describe_move, hand_report, model_fits,
};
use okey_engine::game::{Opened, Step};
use okey_engine::meld::{Shape, pos_tile};
use okey_engine::moves::Move;
use okey_engine::reads::{MAX_PAIRS, Now, Read, read_opponents};
use okey_engine::table::{JokerSlot, LayOption, Side};
use okey_engine::tile::*;
use serde::{Deserialize, Serialize};
use state::{Mode, SEAT_NAMES, Stage, State};
use std::cell::RefCell;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{MessageEvent, Worker};

mod advice;
mod boss;
mod entry;
mod info;
mod table_view;
use advice::*;
use boss::*;
use entry::*;
use info::*;
use table_view::*;

const STORAGE_KEY: &str = "okey101.state.v2";
const UNDO_LIMIT: usize = 150;
const SEAT_OWNER: [&str; 4] = ["kendi", "Sağ'ın", "Karşı'nın", "Sol'un"];
const OPPONENTS: [u8; 3] = [3, 2, 1];
const READ_DEALS: usize = 200;
const GROUP_COLORS: [&str; 6] = ["#22c55e", "#38bdf8", "#f59e0b", "#e879f9", "#f87171", "#a3e635"];

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
                let recorded = replay::on_act(&before, &s, self.shown(&before));
                UNDO.with(|u| {
                    let mut u = u.borrow_mut();
                    u.push(before);
                    if u.len() > UNDO_LIMIT {
                        u.remove(0);
                    }
                    replay::save_undo(&u);
                });
                save_state(&s);
                self.st.set(s);
                self.msg.set(recorded.err());
            }
            Err(e) => self.msg.set(Some(e)),
        }
    }

    /// What the screen showed before an entry: my advice when it was my turn, and the estimates.
    fn shown(&self, s: &State) -> Option<replay::Shown> {
        if s.stage != Stage::Play {
            return None;
        }
        let (advice, deals) = if s.turn == 0 {
            self.analysis.with_untracked(|a| {
                a.as_ref().and_then(|r| r.report.as_ref()).map_or((vec![], 0), |r| {
                    let top = r.candidates.iter().take(3).map(|c| format!("{}: {:.1}", c.label, c.mean)).collect();
                    (top, r.iterations)
                })
            })
        } else {
            (vec![], 0)
        };
        let reads = self.reads.get_untracked().map_or(vec![], |rs| {
            [1u8, 2, 3]
                .iter()
                .filter_map(|&p| rs.iter().find(|r| r.seat == p))
                .map(|r| read_lines(r, s).join(" · "))
                .collect()
        });
        Some(replay::Shown { advice, deals, reads })
    }

    fn undo(&self) {
        let cur = self.st.get_untracked();
        if let Some(prev) = UNDO.with(|u| {
            let mut u = u.borrow_mut();
            let prev = u.pop();
            replay::save_undo(&u);
            prev
        }) {
            replay::on_undo(&cur, &prev);
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
    let fake = t != JOKER && Some(t) == okey;
    let title = if t == JOKER {
        "Okey".to_string()
    } else if fake {
        format!("Sahte okey ({} yerine)", name(t))
    } else {
        name(t)
    };
    view! {
        <span class=format!("t {}", color_class(t)) class:sahte=fake title=title>
            {if t == JOKER { "★".to_string() } else { num(t).to_string() }}
        </span>
    }
}

fn tiles_view(tiles: &[Tile], okey: Option<Tile>) -> impl IntoView + use<> {
    tiles.iter().map(|&t| tile_view(t, okey)).collect_view()
}

fn okey_tile(s: &State) -> Option<Tile> {
    s.indicator.map(okey_of)
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
    UNDO.with(|u| *u.borrow_mut() = replay::load_undo());
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
    let now_key = Memo::new(move |_| {
        st.with(|s| Now {
            seat: s.turn,
            drawn: s.may_act(),
            taken: s.taken,
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
            let now = now_key.get();
            view_key
                .get()
                .and_then(|j| serde_json::from_str::<View>(&j).ok())
                .map(|v| read_opponents(&v, now, READ_DEALS, 11))
        }),
    };
    provide_context(ctx);

    for i in 0..worker_count() {
        let Some(url) = worker_url() else { break };
        let Ok(w) = Worker::new(&url) else { break };
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
                    <h3>"Kayıtlı eller"</h3>
                    <Replays/>
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
                let held_indicator = s.indicator.filter(|_| s.stage != Stage::Setup).map(|g| view! {
                    <span class="chip"><span class="lbl">"Gösterge "</span>{tile_view(g, None)}<span class="lbl">" okey "</span><span class="arrow">"→"</span>{tile_view(okey_of(g), None)}</span>
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
                view! { {head} {deck} {held_indicator} {game} }
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
