mod settings;
mod state;

use leptos::prelude::*;
use okey_engine::analysis::{
    CandReport, Decision, HandReport, Options, Phase, Report, describe, describe_move, hand_report, merge_reports,
};
use okey_engine::game::{Opened, Step};
use okey_engine::meld::{Shape, pos_tile};
use okey_engine::moves::Move;
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

#[derive(Deserialize, Clone, Debug)]
struct Resp {
    generation: u32,
    report: Option<Report>,
    error: Option<String>,
    done: bool,
}

#[derive(Serialize)]
#[serde(tag = "kind")]
enum Req<'a> {
    Start {
        generation: u32,
        view: &'a okey_engine::analysis::View,
        seed: u64,
        options: Options,
    },
    Stop,
}

thread_local! {
    static WORKERS: RefCell<Vec<Worker>> = const { RefCell::new(Vec::new()) };
    static LATEST: RefCell<Vec<Option<Resp>>> = const { RefCell::new(Vec::new()) };
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

/// Start the same analysis on every worker, each with its own random deals.
fn start_all(generation: u32, view: &okey_engine::analysis::View) {
    let base = js_sys::Date::now() as u64;
    LATEST.with(|l| l.borrow_mut().iter_mut().for_each(|r| *r = None));
    WORKERS.with(|ws| {
        for (i, w) in ws.borrow().iter().enumerate() {
            let req = Req::Start {
                generation,
                view,
                seed: base.wrapping_add(i as u64 * 0x9E37_79B9),
                options: Options::default(),
            };
            if let Ok(j) = serde_json::to_string(&req) {
                let _ = w.post_message(&JsValue::from_str(&j));
            }
        }
    });
}

fn stop_all() {
    WORKERS.with(|ws| {
        for w in ws.borrow().iter() {
            if let Ok(j) = serde_json::to_string(&Req::Stop) {
                let _ = w.post_message(&JsValue::from_str(&j));
            }
        }
    });
}

/// Fold every worker's latest report for the current position into one.
fn merged(generation: u32) -> Option<Resp> {
    LATEST.with(|l| {
        let l = l.borrow();
        let current: Vec<&Resp> = l.iter().flatten().filter(|r| r.generation == generation).collect();
        if current.is_empty() {
            return None;
        }
        if let Some(e) = current.iter().find_map(|r| r.error.clone()) {
            return Some(Resp {
                generation,
                report: None,
                error: Some(e),
                done: true,
            });
        }
        let reports: Vec<Report> = current.iter().filter_map(|r| r.report.clone()).collect();
        let done = current.len() == l.len() && current.iter().all(|r| r.done);
        Some(Resp {
            generation,
            report: Some(merge_reports(&reports)),
            error: None,
            done,
        })
    })
}

#[derive(Clone, Copy)]
pub struct Ctx {
    pub st: RwSignal<State>,
    msg: RwSignal<Option<String>>,
    analysis: RwSignal<Option<Resp>>,
    color: RwSignal<u8>,
    pub show_settings: RwSignal<bool>,
    /// An alternative the player picked over the top recommendation, with its steps.
    pick: RwSignal<Option<(Decision, Option<Move>)>>,
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

fn tile_class(t: Tile, okey: Option<Tile>) -> String {
    if t == JOKER {
        "tile joker".into()
    } else if Some(t) == okey {
        format!("tile c{} sahte", color(t))
    } else {
        format!("tile c{}", color(t))
    }
}

fn tile_text(t: Tile) -> String {
    if t == JOKER { "OK".into() } else { num(t).to_string() }
}

fn tile_view(t: Tile, okey: Option<Tile>) -> impl IntoView + use<> {
    view! { <span class=tile_class(t, okey) title=name(t)>{tile_text(t)}</span> }
}

fn tiles_view(tiles: &[Tile], okey: Option<Tile>) -> impl IntoView + use<> {
    tiles.iter().map(|&t| tile_view(t, okey)).collect_view()
}

fn okey_tile(s: &State) -> Option<Tile> {
    s.gosterge.map(okey_of)
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

/// Which tiles the okeys in a meld stand for, as a short note.
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

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let ctx = Ctx {
        st: RwSignal::new(load_state()),
        msg: RwSignal::new(None),
        analysis: RwSignal::new(None),
        color: RwSignal::new(0),
        show_settings: RwSignal::new(false),
        pick: RwSignal::new(None),
    };
    provide_context(ctx);

    let analysis = ctx.analysis;
    for i in 0..worker_count() {
        let Ok(w) = Worker::new("./worker_loader.js") else { break };
        let onmsg = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            let Some(s) = e.data().as_string() else { return };
            let Ok(r) = serde_json::from_str::<Resp>(&s) else { return };
            let generation = GENERATION.with(|g| *g.borrow());
            if r.generation != generation {
                return;
            }
            LATEST.with(|l| l.borrow_mut()[i] = Some(r));
            if let Some(m) = merged(generation) {
                analysis.set(Some(m));
            }
        });
        w.set_onmessage(Some(onmsg.as_ref().unchecked_ref()));
        onmsg.forget();
        WORKERS.with(|x| x.borrow_mut().push(w));
        LATEST.with(|l| l.borrow_mut().push(None));
    }

    Effect::new(move |_| {
        let view = ctx.st.with(|s| s.view());
        let key = view.as_ref().and_then(|v| serde_json::to_string(v).ok()).unwrap_or_default();
        if LAST_SENT.with(|l| *l.borrow() == key) {
            return;
        }
        LAST_SENT.with(|l| *l.borrow_mut() = key);
        let generation = GENERATION.with(|g| {
            *g.borrow_mut() += 1;
            *g.borrow()
        });
        analysis.set(None);
        ctx.pick.set(None);
        match view {
            Some(v) => start_all(generation, &v),
            None => stop_all(),
        }
    });

    let st = ctx.st;
    view! {
        <div class="app" class:kbd-on=move || st.with(|s| s.keyboard_visible())>
            <Header/>
            <Show when=move || st.with(|s| s.stage != Stage::Setup)>
                <Seats/>
            </Show>
            <main>
                <Message/>
                <Show when=move || ctx.show_settings.get()>
                    <settings::Settings/>
                </Show>
                {move || match st.with(|s| (s.stage, s.turn == 0)).0 {
                    Stage::Setup => view! { <Setup/> }.into_any(),
                    Stage::Play if st.with(|s| s.turn == 0) => view! { <Wizard/> <MyRack/> <TableView/> <Details/> }.into_any(),
                    Stage::Play => view! { <Wizard/> <TableView/> <MyRack/> <Details/> }.into_any(),
                    Stage::Over => view! { <Over/> <TableView/> }.into_any(),
                }}
            </main>
            <Show when=move || st.with(|s| s.keyboard_visible())>
                <Keyboard/>
            </Show>
        </div>
    }
}

#[component]
fn Header() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    view! {
        <header>
            <span class="brand">"101"</span>
            <span class="deck">
                {move || ctx.st.with(|s| if s.stage == Stage::Setup { String::new() } else { format!("Deste {}", s.deck_count) })}
            </span>
            <button on:click=move |_| ctx.undo()>"↶ Geri al"</button>
            <button on:click=move |_| ctx.show_settings.update(|v| *v = !*v)>"Kurallar"</button>
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
                (0..4usize).map(|p| {
                    let pv = &s.players[p];
                    let count = if p == 0 { s.hand.len() as u8 } else { pv.hand_len.unwrap_or(0) };
                    let badge = match pv.opened {
                        Opened::No => String::new(),
                        Opened::Melds => format!("açtı {}", pv.open_value),
                        Opened::Pairs => format!("{} çift", pv.open_value),
                    };
                    let cur = s.stage == Stage::Play && s.turn as usize == p;
                    view! {
                        <div class="seat" class:cur=cur>
                            <b>{SEAT_NAMES[p]}</b>
                            <span>{format!("{count} taş")}</span>
                            <small>{badge}</small>
                        </div>
                    }
                }).collect_view()
            }}
        </div>
    }
}

#[component]
fn Message() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    view! {
        <Show when=move || ctx.msg.with(|m| m.is_some())>
            <div class="msg" on:click=move |_| ctx.msg.set(None)>{move || ctx.msg.get().unwrap_or_default()}</div>
        </Show>
    }
}

#[component]
fn Setup() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    view! {
        <section class="card ask">
            {move || {
                let s = st.get();
                let okey = okey_tile(&s);
                let gost = s.gosterge.map(|g| view! {
                    <button class="chip" on:click=move |_| ctx.mode(Mode::Gosterge)>"Gösterge " {tile_view(g, None)} " · okey " {tile_view(okey_of(g), None)}</button>
                });
                match s.mode {
                    Mode::Starter => view! {
                        <div class="step">"2 / 3"</div>
                        <h2>"Kim başlıyor?"</h2>
                        <div class="row">{gost}</div>
                        <div class="choices">
                            {(0..4u8).map(|p| view! {
                                <button class="big" on:click=move |_| ctx.act(|s| { s.set_starter(p); Ok(()) })>{SEAT_NAMES[p as usize]}</button>
                            }).collect_view()}
                        </div>
                    }.into_any(),
                    Mode::Hand => {
                        let full = s.hand.len() == s.expected_hand();
                        view! {
                            <div class="step">"3 / 3"</div>
                            <h2>{format!("Taşlarını gir · {} / {}", s.hand.len(), s.expected_hand())}</h2>
                            <div class="row">
                                {gost}
                                <button class="chip" on:click=move |_| ctx.mode(Mode::Starter)>{format!("Başlayan: {}", SEAT_NAMES[s.starter as usize])}</button>
                            </div>
                            <div class="rack">
                                {s.hand.iter().enumerate().map(|(i, &t)| view! {
                                    <button class="tbtn" on:click=move |_| ctx.act(|s| { s.remove_from_hand(i); Ok(()) })>{tile_view(t, okey)}</button>
                                }).collect_view()}
                            </div>
                            {if full {
                                view! { <button class="primary big wide" on:click=move |_| ctx.act(|s| s.start())>"Başla"</button> }.into_any()
                            } else {
                                view! { <div class="hint">"Yanlış girdiğin taşa dokunursan silinir."</div> }.into_any()
                            }}
                        }.into_any()
                    }
                    _ => view! {
                        <div class="step">"1 / 3"</div>
                        <h2>"Gösterge taşı hangisi?"</h2>
                        <div class="hint">"Ortada açık duran taş. Aşağıdan rengini ve sayısını seç."</div>
                    }.into_any(),
                }
            }}
        </section>
    }
}

#[component]
fn Over() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    view! {
        <section class="card ask">
            <h2>{move || ctx.st.with(|s| s.result.clone().unwrap_or_default())}</h2>
            <button class="primary big wide" on:click=move |_| ctx.act(|s| { *s = s.new_hand(); Ok(()) })>"Yeni el"</button>
        </section>
    }
}

/// The one card that asks for the next thing.
#[component]
fn Wizard() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    move || {
        let s = st.get();
        let me = s.turn == 0;
        match s.mode {
            Mode::MeldEntry => meld_entry(ctx, &s).into_any(),
            Mode::Manual if me => manual(ctx, &s).into_any(),
            Mode::MeldMenu(i) => meld_menu(ctx, &s, i).into_any(),
            Mode::PairEntry { owner, okey } => pair_entry(ctx, &s, owner, okey).into_any(),
            Mode::PickDraw => view! {
                <section class="card ask">
                    <h2>"Hangi taşı çektin?"</h2>
                    <div class="hint">"Aşağıdan seç."</div>
                    <button on:click=move |_| ctx.mode(Mode::Idle)>"Vazgeç"</button>
                </section>
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

fn opp_draw(ctx: Ctx, s: &State) -> impl IntoView + use<> {
    let who = SEAT_NAMES[s.turn as usize];
    let side = s.side_tile();
    let okey = okey_tile(s);
    let deck = s.deck_count > 0;
    view! {
        <section class="card ask">
            <h2>{format!("{who} taşı nereden aldı?")}</h2>
            <div class="choices">
                {deck.then(|| view! { <button class="primary big" on:click=move |_| ctx.act(|s| s.opp_draw())>"Desteden çekti"</button> })}
                {side.map(|t| view! {
                    <button class="big" on:click=move |_| ctx.act(|s| s.take_side())>"Yandan aldı " {tile_view(t, okey)}</button>
                })}
                {(!deck).then(|| view! { <button class="big danger" on:click=move |_| ctx.act(|s| { s.deck_out(); Ok(()) })>"Deste bitti · el kapandı"</button> })}
            </div>
        </section>
    }
}

fn opp_act(ctx: Ctx, s: &State) -> impl IntoView + use<> {
    let who = SEAT_NAMES[s.turn as usize];
    let okey = okey_tile(s);
    let pv = s.cur();
    let must = s.taken.map(|t| view! {
        <div class="warn">"Yandan aldığı " {tile_view(t, okey)} " taşını bu tur kullanmak zorunda. Önce indirdiği peri gir ya da işlediği pere dokun."</div>
    });
    let opened_now = s.opened_this_turn.then(|| {
        let what = if pv.opened == Opened::Pairs {
            format!("{} çift", pv.open_value)
        } else {
            format!("{} puan", pv.open_value)
        };
        view! { <div class="good">{format!("Bu tur açtı: {what}")}</div> }
    });
    let can_tap = s.can_lay_off() && (0..s.table.melds.len()).any(|i| s.meld_tappable(i));
    let short = (!s.opening_complete()).then(|| {
        view! {
            <div class="warn">{s.opening_progress()} " · açılış eksik. Kalan perleri gir; yanlış girdiysen Geri al."</div>
        }
    });
    view! {
        <section class="card ask">
            <h2>{format!("{who} hangi taşı attı?")}</h2>
            {must}
            {opened_now}
            {short}
            <div class="hint">"Aşağıdan attığı taşa dokun."</div>
            <div class="row">
                <button on:click=move |_| ctx.mode(Mode::MeldEntry)>"Önce per / çift indirdi"</button>
                <button class="danger" on:click=move |_| ctx.act(|s| { let p = s.turn; s.finish(p, None); Ok(()) })>"Bitirdi"</button>
            </div>
            {can_tap.then(|| view! { <div class="hint">"İşlediyse ya da okey aldıysa: masada o pere dokun."</div> })}
        </section>
    }
}

fn meld_entry(ctx: Ctx, s: &State) -> impl IntoView + use<> {
    let who = SEAT_NAMES[s.turn as usize];
    let okey = okey_tile(s);
    let problem = s.meld_problem();
    let ok = problem.is_none();
    let status = match (&problem, s.buffer_shape()) {
        (None, Some(sh)) if sh.is_pair() => "Çift ✓".to_string(),
        (None, Some(sh)) => format!("{} ✓ · {} puan", shape_label(&sh), sh.open_value(&s.rules)),
        (Some(p), _) => p.clone(),
        _ => String::new(),
    };
    let total = s.opening_progress();
    let total_class = if s.opening_complete() { "good" } else { "warn" };
    let buffer = s.buffer.clone();
    view! {
        <section class="card ask">
            <h2>{format!("{who} hangi taşları indirdi?")}</h2>
            <div class="hint">"Tek bir perin taşlarına sırayla dokun, sonra Ekle. Birden fazla per varsa tek tek ekle."</div>
            <div class="rack">
                {buffer.into_iter().enumerate().map(|(i, t)| view! {
                    <button class="tbtn" on:click=move |_| ctx.act(|s| { s.buffer.remove(i); Ok(()) })>{tile_view(t, okey)}</button>
                }).collect_view()}
            </div>
            <div class=if ok { "good" } else { "hint" }>{status}</div>
            {total.map(|t| view! { <div class=total_class>{t}</div> })}
            <div class="choices">
                <button class="primary big" disabled=!ok on:click=move |_| ctx.act(|s| s.lay_meld())>"Ekle"</button>
                <button class="big" on:click=move |_| ctx.mode(Mode::Idle)>"Tamam, bitti"</button>
            </div>
        </section>
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

fn meld_menu(ctx: Ctx, s: &State, idx: usize) -> impl IntoView + use<> {
    let okey = okey_tile(s);
    let me = s.turn == 0;
    let who = SEAT_NAMES[s.turn as usize];
    let Some(m) = s.table.melds.get(idx).cloned() else {
        return view! { <section class="card ask"><button on:click=move |_| ctx.mode(Mode::Idle)>"Geri"</button></section> }.into_any();
    };
    let lays = s.lay_options(idx);
    let swaps: Vec<JokerSlot> = s.swap_options(idx);
    let pair = m.shape.is_pair() && s.can_add_pair(m.owner);
    let owner = m.owner;
    let foreign = owner != s.turn && !(s.rules.partners && owner == (s.turn + 2) % 4);
    let penalty = s.rules.okey_lost_penalty;
    let nothing = lays.is_empty() && swaps.is_empty() && !pair;
    view! {
        <section class="card ask">
            <h2>{if me { "Bu pere ne yapıyorsun?".to_string() } else { format!("{who} bu pere ne yaptı?") }}</h2>
            <div class="meld">{tiles_view(&m.shape.tiles(), okey)}</div>
            <div class="options">
                {lays.into_iter().map(|o| view! {
                    <button class="opt" on:click=move |_| ctx.act(|s| s.lay_option(idx, o))>
                        {tile_view(o.tile, okey)} <span>{option_label(&o)}</span>
                    </button>
                }).collect_view()}
                {swaps.into_iter().map(|slot| {
                    let give = slot.tiles.clone();
                    let n = slot.jokers;
                    view! {
                        <button class="opt" on:click=move |_| ctx.act(|s| s.swap(&slot))>
                            {tiles_view(&give, okey)}
                            <span>{format!("koyup {} al", if n > 1 { "2 okeyi" } else { "okeyi" })}{foreign.then(|| format!(" · per sahibine {penalty} ceza"))}</span>
                        </button>
                    }
                }).collect_view()}
                {pair.then(|| view! {
                    <button class="opt" on:click=move |_| ctx.mode(Mode::PairEntry { owner, okey: false })>"Buraya çift işle"</button>
                })}
            </div>
            {nothing.then(|| view! { <div class="hint">"Bu pere şu an eklenebilecek bir şey yok (uç başına turda en fazla 2 taş)."</div> })}
            <button class="big wide" on:click=move |_| ctx.mode(if me { Mode::Manual } else { Mode::Idle })>"Tamam"</button>
        </section>
    }.into_any()
}

fn pair_entry(ctx: Ctx, s: &State, owner: u8, okey: bool) -> impl IntoView + use<> {
    let me = s.turn == 0;
    let room = s.table.pair_room(owner, &s.rules);
    view! {
        <section class="card ask">
            <h2>{format!("{} çiftlerine hangi çift?", SEAT_OWNER[owner as usize])}</h2>
            <div class="hint">{format!("Aşağıdan taşa dokun; çift olarak eklenir. Bu tur {room} çift daha işlenebilir.")}</div>
            <div class="choices">
                <button class="big" class:active=okey on:click=move |_| ctx.mode(Mode::PairEntry { owner, okey: !okey })>
                    {if okey { "Okeyli çift: AÇIK" } else { "Okeyli çift: kapalı" }}
                </button>
                <button class="big" on:click=move |_| ctx.mode(if me { Mode::Manual } else { Mode::Idle })>"Tamam"</button>
            </div>
        </section>
    }
}

fn manual(ctx: Ctx, s: &State) -> impl IntoView + use<> {
    let okey = okey_tile(s);
    let n = s.buffer.len();
    let problem = s.meld_problem();
    let can_meld = problem.is_none() && n >= 2;
    let status = match (n, &problem, s.buffer_shape()) {
        (0, _, _) => "Elinden taşlara dokun.".to_string(),
        (1, _, _) => "Tek taş seçili: atabilirsin. İşlemek için masadaki pere dokun.".to_string(),
        (_, None, Some(sh)) if sh.is_pair() => "Çift ✓".to_string(),
        (_, None, Some(sh)) => format!("{} ✓ · {} puan", shape_label(&sh), sh.open_value(&s.rules)),
        (_, Some(p), _) => p.clone(),
        _ => String::new(),
    };
    let complete = s.opening_complete();
    let single = (n == 1 && complete).then(|| s.buffer[0]);
    let buffer = s.buffer.clone();
    let short = (!complete).then(|| view! {
        <div class="warn">{s.opening_progress()} " · açılış tamamlanmadan işleyemez, okey alamaz, taş atamazsın. Eksik perleri indir ya da Geri al."</div>
    });
    view! {
        <section class="card ask">
            <h2>"Kendi hamlen"</h2>
            {short}
            <div class="rack">{tiles_view(&buffer, okey)}</div>
            <div class=if can_meld { "good" } else { "hint" }>{status}</div>
            <div class="choices">
                {can_meld.then(|| view! { <button class="primary big" on:click=move |_| ctx.act(|s| s.lay_meld())>"İndir"</button> })}
                {single.map(|t| view! {
                    <button class="primary big" on:click=move |_| ctx.act(|s| { s.buffer.clear(); s.discard(0, t) })>"Bu taşı at " {tile_view(t, okey)}</button>
                })}
                {complete.then(|| view! { <button class="big" on:click=move |_| ctx.mode(Mode::Idle)>"Öneriye dön"</button> })}
            </div>
        </section>
    }
}

fn find(r: &Report, d: Decision) -> Option<&CandReport> {
    r.candidates.iter().find(|c| c.decision == d)
}

/// The expected score gap in words: how far ahead of (or behind) the opponents this move leaves me.
fn advantage(c: &CandReport) -> String {
    let lead = -c.mean;
    if lead >= 0.0 {
        format!("{lead:.0} puan öndesin")
    } else {
        format!("{:.0} puan geridesin", -lead)
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
        let advice = match (&draw, &take) {
            (Some(d), Some(t)) => {
                let diff = (d.mean - t.mean).abs();
                let pick = if t.mean < d.mean { "Yandan al" } else { "Desteden çek" };
                format!("Öneri: {pick} ({diff:.1} puan daha iyi)")
            }
            (Some(_), None) if side.is_some() && !take_ok => "Yandaki taşı bu tur kullanamazsın, alınamaz.".to_string(),
            (None, None) if take_ok => "Hesaplanıyor…".to_string(),
            _ => String::new(),
        };
        view! {
            <section class="card ask">
                <h2>"Sıra sende · taş çek"</h2>
                <div class="hint">{advice}</div>
                <div class="choices">
                    {deck.then(|| view! {
                        <button class="big" class:primary=!best_take on:click=move |_| ctx.act(|s| s.apply(Decision::Draw))>"Desteden çektim"</button>
                    })}
                    {side.filter(|_| take_ok).map(|t| view! {
                        <button class="big" class:primary=best_take on:click=move |_| ctx.act(|s| s.apply(Decision::Take))>"Yandan aldım " {tile_view(t, okey)}</button>
                    })}
                    {(!deck && !take_ok).then(|| view! {
                        <button class="big danger" on:click=move |_| ctx.act(|s| { s.deck_out(); Ok(()) })>"Deste bitti · el kapandı"</button>
                    })}
                </div>
            </section>
        }
    }
}

fn step_view(step: &Step, n: usize, okey: Option<Tile>, s: &State) -> impl IntoView + use<> {
    let num = view! { <span class="n">{n}</span> };
    match step.clone() {
        Step::Meld { shape, owner } => {
            let tiles = shape.tiles();
            let head = if shape.is_pair() {
                if owner == 0 { "Çift indir".to_string() } else { format!("{} çiftlerine işle", SEAT_OWNER[owner as usize]) }
            } else {
                format!("{} indir · {} puan", shape_label(&shape), shape.open_value(&s.rules))
            };
            let note = okey_note(&shape);
            view! {
                <li>{num}<div><b>{head}</b><div class="meld">{tiles_view(&tiles, okey)}</div>{note.map(|n| view! { <small>{n}</small> })}</div></li>
            }.into_any()
        }
        Step::Layoff { tile, owner, onto, side, stands_for } => {
            let whose = if owner == 0 { "kendi".to_string() } else { SEAT_OWNER[owner as usize].to_string() };
            let place = match side {
                Side::Group => "perine ekle".to_string(),
                side => format!("perinin {}", side_text(side).to_uppercase()),
            };
            let note = (tile == JOKER && stands_for != JOKER).then(|| format!("okey = {}", name(stands_for)));
            view! {
                <li>{num}<div>
                    <b>"İşle: " {tile_view(tile, okey)} {format!(" → {whose} {place}")}</b>
                    <div class="meld">{tiles_view(&onto, okey)}</div>
                    {note.map(|n| view! { <small>{n}</small> })}
                </div></li>
            }.into_any()
        }
        Step::Swap { owner, onto, give, jokers } => {
            let foreign = owner != 0 && !(s.rules.partners && owner == 2);
            let pen = foreign.then(|| format!(" · ona {} ceza yazılır", s.rules.okey_lost_penalty));
            view! {
                <li>{num}<div>
                    <b>"Okey al: " {tiles_view(&give, okey)} {format!(" koy, {} perinden {} okey al", SEAT_OWNER[owner as usize], jokers)}{pen}</b>
                    <div class="meld">{tiles_view(&onto, okey)}</div>
                </div></li>
            }.into_any()
        }
        Step::Discard { tile, finish } => view! {
            <li>{num}<div><b>{if finish { "Son taşı at: " } else { "At: " }} {tile_view(tile, okey)}{finish.then_some(" · el biter!")}</b></div></li>
        }.into_any(),
    }
}

#[component]
fn Plan() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    let best = Memo::new(move |_| {
        ctx.analysis.with(|a| {
            a.as_ref()
                .and_then(|r| r.report.as_ref())
                .and_then(|r| r.candidates.first().map(|c| (c.decision, c.mv.clone())))
        })
    });
    let chosen = Memo::new(move |_| ctx.pick.get().or(best.get()));
    let steps = Memo::new(move |_| {
        let (d, mv) = chosen.get()?;
        let v = st.with(|s| s.view())?;
        Some(match &mv {
            Some(m) => describe_move(&v, m),
            None => describe(&v, d),
        })
    });
    view! {
        <section class="card ask plan">
            {move || {
                let s = st.get();
                let okey = okey_tile(&s);
                let resp = ctx.analysis.get();
                if let Some(Resp { error: Some(e), .. }) = &resp {
                    return view! { <div class="msg">{e.clone()}</div> }.into_any();
                }
                let Some((d, mv)) = chosen.get() else {
                    return view! { <h2>"Sıra sende"</h2><div class="hint">"En iyi hamle hesaplanıyor…"</div> }.into_any();
                };
                let report = resp.as_ref().and_then(|r| r.report.clone());
                let cand = report.as_ref().and_then(|r| find(r, d).cloned());
                let is_best = best.get().is_some_and(|(b, _)| b == d);
                let screening = report.as_ref().is_some_and(|r| r.screening);
                // Settled once the runner-up is clearly behind, or the search has run its course.
                let (settled, close) = match (&resp, &report) {
                    (Some(r), Some(rep)) => {
                        let mut alive = rep.candidates.iter().filter(|c| c.alive);
                        let top = alive.next();
                        let second = alive.next();
                        let apart = match (top, second) {
                            (Some(t), Some(s)) => s.mean - t.mean > 2.0 * s.diff_se,
                            _ => true,
                        };
                        (r.done || (apart && top.is_some_and(|t| t.n >= 300)), !apart)
                    }
                    _ => (false, false),
                };
                let caution = if screening {
                    Some("Bütün hamleler taranıyor: öneri değişebilir.")
                } else if !settled {
                    Some("Henüz kesin değil: hesap sürüyor, öneri değişebilir.")
                } else if close && is_best {
                    Some("İlk iki seçenek neredeyse eşit; ikisi de olur.")
                } else {
                    None
                };
                let status = resp.as_ref().zip(report.as_ref()).map(|(r, rep)| {
                    let moves = if rep.positions > 0 { format!("{} farklı indirme şekli, her biri için atılacak her taş · ", rep.positions) } else { String::new() };
                    let cut = if rep.truncated { " · seçenek çok fazla, bir kısmı incelenemedi" } else { "" };
                    let deep = if rep.deep { " · sonraki turun da hesaba katılıyor" } else { "" };
                    format!("{moves}{} senaryo{}{cut}{deep}", rep.iterations, if r.done { " · tamam" } else { " · netleşiyor…" })
                });
                let stats = cand.as_ref().map(|c| format!(
                    "Beklenen: rakiplerden {} · bitirme {:.0}% · sağdaki attığın taşı alır {:.0}%",
                    advantage(c), c.p_finish * 100.0, c.p_next_takes * 100.0
                ));
                let list = steps.get().unwrap_or_default();
                let none_to_lay = list.len() <= 1;
                view! {
                    <h2>{if is_best { "Öneri" } else { "Seçtiğin hamle" }}</h2>
                    {caution.map(|c| view! { <div class="warn">{c}</div> })}
                    <div class="hint">{stats}</div>
                    <ol class="steps">
                        {none_to_lay.then(|| view! { <li class="plain"><span class="n">"·"</span><div>"Per indirme, işleme yok."</div></li> })}
                        {list.iter().enumerate().map(|(i, st)| step_view(st, i + 1, okey, &s)).collect_view()}
                    </ol>
                    <button class="primary big wide" on:click=move |_| ctx.act(|s| match &mv {
                        Some(m) => s.apply_move(m),
                        None => s.apply(d),
                    })>"Yaptım ✓"</button>
                    <div class="hint small">{status}</div>
                }.into_any()
            }}
            <Alternatives/>
            <button class="wide" on:click=move |_| ctx.mode(Mode::Manual)>"Başka bir şey yaptım / kendim gireceğim"</button>
        </section>
    }
}

#[component]
fn Alternatives() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    view! {
        <details class="alts">
            <summary>"Diğer seçenekler"</summary>
            {move || {
                let Some(report) = ctx.analysis.get().and_then(|r| r.report) else { return ().into_any() };
                let Some(top) = report.candidates.first().cloned() else { return ().into_any() };
                let picked = ctx.pick.get().map_or(top.decision, |(d, _)| d);
                report.candidates.iter().filter(|c| c.mv.is_some() || !matches!(c.decision, Decision::Play(_))).take(10).map(|c| {
                    let d = c.decision;
                    let mv = c.mv.clone();
                    let behind = c.mean - top.mean;
                    let note = if behind < 0.05 {
                        "en iyi".to_string()
                    } else if c.alive {
                        format!("{behind:.1} geride (±{:.1})", c.diff_se)
                    } else {
                        format!("elendi · ~{behind:.0} geride")
                    };
                    let note = format!("{note} · sağ alır {:.0}%", c.p_next_takes * 100.0);
                    view! {
                        <button class="alt" class:active=picked == d on:click=move |_| ctx.pick.set(Some((d, mv.clone())))>
                            <span>{c.label.clone()}</span>
                            <small>{note}</small>
                        </button>
                    }
                }).collect_view().into_any()
            }}
        </details>
    }
}

#[component]
fn MyRack() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    view! {
        <section class="card">
            <h3>{move || st.with(|s| format!("Elim · {} taş", s.hand.len()))}</h3>
            <div class="rack">
                {move || {
                    let s = st.get();
                    let okey = okey_tile(&s);
                    let active = s.turn == 0 && s.may_act();
                    let mut used = if s.mode == Mode::Manual { s.buffer.clone() } else { vec![] };
                    s.hand.iter().map(|&t| {
                        let sel = used.iter().position(|&b| b == t).map(|i| used.remove(i)).is_some();
                        view! {
                            <button class="tbtn" class:sel=sel disabled=!active on:click=move |_| ctx.act(|s| {
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
                            })>{tile_view(t, okey)}</button>
                        }
                    }).collect_view()
                }}
            </div>
        </section>
    }
}

#[component]
fn TableView() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    view! {
        <section class="card">
            <h3>"Masa"</h3>
            {move || {
                let s = st.get();
                if s.table.melds.is_empty() {
                    return view! { <div class="muted">"Henüz açan yok."</div> }.into_any();
                }
                let okey = okey_tile(&s);
                (0..4u8).filter(|&o| s.table.melds.iter().any(|m| m.owner == o)).map(|o| {
                    let melds: Vec<_> = s.table.melds.iter().enumerate().filter(|(_, m)| m.owner == o).map(|(i, m)| (i, m.clone(), s.meld_tappable(i))).collect();
                    view! {
                        <div class="owner">
                            <div class="muted small">{SEAT_NAMES[o as usize]}</div>
                            {melds.into_iter().map(|(i, m, tap)| view! {
                                <button class="meld mbtn" class:tap=tap disabled=!tap on:click=move |_| ctx.act(|s| { s.buffer.clear(); s.mode = Mode::MeldMenu(i); Ok(()) })>
                                    {tiles_view(&m.shape.tiles(), okey)}
                                </button>
                            }).collect_view()}
                        </div>
                    }
                }).collect_view().into_any()
            }}
        </section>
    }
}

#[component]
fn Details() -> impl IntoView {
    view! {
        <details class="card">
            <summary>"Atılan taşlar"</summary>
            <Discards/>
        </details>
        <details class="card">
            <summary>"Elimin durumu · işime yarayacak taşlar"</summary>
            <HandInfo/>
        </details>
        <details class="card">
            <summary>"Görülmemiş taşlar"</summary>
            <Unseen/>
        </details>
        <CopyState/>
    }
}

/// Copies the whole position as text so a questionable recommendation can be reproduced exactly.
#[component]
fn CopyState() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let copied = RwSignal::new(false);
    view! {
        <button class="wide" on:click=move |_| {
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
        (0..4usize)
            .map(|p| {
                let pv = &s.players[p];
                let known = (!pv.known.is_empty()).then(|| {
                    view! {
                        <span class="muted small">" elinde: "</span>
                        {tiles_view(&pv.known, okey)}
                    }
                });
                view! {
                    <div class="disc-row">
                        <span class="seatname">{SEAT_NAMES[p]}</span>
                        <span class="disc">{tiles_view(&pv.discards, okey)}</span>
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
    let report = Memo::new(move |_| st.with(|s| s.view_as(Phase::AfterDraw).map(|v| hand_report(&v))));
    move || {
        let okey = okey_tile(&st.get());
        report.get().map(|r: HandReport| {
            let opened = st.with(|s| s.players[0].opened);
            let head = match opened {
                Opened::No => format!("Perle açış: {} / {} · Çift: {} / {}", r.open_value, r.meld_req, r.pairs, r.pair_req),
                Opened::Melds => "Perle açtın.".to_string(),
                Opened::Pairs => "Çiftle açtın.".to_string(),
            };
            view! {
                <div>{head}</div>
                <div class="rack">
                    {r.melds.iter().map(|m| view! { <span class="meld">{tiles_view(m, okey)}</span> }).collect_view()}
                </div>
                <div class="muted small">"Taş × görülmemiş adet → gelirse açış değerin / çift sayın"</div>
                <div class="outs">
                    {r.outs.iter().take(16).map(|o| view! {
                        <span class="out">{tile_view(o.tile, okey)}<span class="small">{format!("×{} → {} / {}ç", o.unseen, o.open_value, o.pairs)}</span></span>
                    }).collect_view()}
                </div>
            }
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
        view! {
            <div class="muted small">{format!("Toplam {total} taş (rakiplerin eli + deste)")}</div>
            <table class="grid">
                {(0..4u8).map(|c| view! {
                    <tr>
                        {(1..=13u8).map(|n| {
                            let t = id(c, n);
                            view! { <td class=format!("u{}", left(t))>{tile_view(t, okey)}<sub>{left(t)}</sub></td> }
                        }).collect_view()}
                    </tr>
                }).collect_view()}
                <tr><td class=format!("u{}", left(JOKER))>{tile_view(JOKER, okey)}<sub>{left(JOKER)}</sub></td></tr>
            </table>
        }
    }
}

#[component]
fn Keyboard() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    let color_sel = ctx.color;
    view! {
        <nav class="kbd">
            <div class="colors">
                {(0..4u8).map(|c| view! {
                    <button class=format!("cbtn c{c}") class:active=move || color_sel.get() == c on:click=move |_| color_sel.set(c)>
                        {COLOR_NAMES[c as usize]}
                    </button>
                }).collect_view()}
            </div>
            <div class="nums">
                {(1..=14u8).map(|n| {
                    let tile = move || if n == 14 { JOKER } else { id(color_sel.get(), n) };
                    let off = move || st.with(|s| !s.key_enabled(tile()));
                    view! {
                        <button
                            class=move || if n == 14 { "key joker".to_string() } else { format!("key c{}", color_sel.get()) }
                            class:gone=off
                            disabled=off
                            on:click=move |_| ctx.act(|s| s.key(tile()))
                        >
                            {if n == 14 { "OKEY".to_string() } else { n.to_string() }}
                            {move || st.with(|s| (n != 14 && okey_tile(s) == Some(tile())).then(|| view! { <small>"sahte"</small> }))}
                        </button>
                    }
                }).collect_view()}
            </div>
        </nav>
    }
}
