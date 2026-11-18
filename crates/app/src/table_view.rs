//! The seats, the table, my rack and the tile keyboard.

use super::*;

#[component]
pub(super) fn Seats() -> impl IntoView {
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
                    let guess = ctx.reads.get().and_then(|rs| rs.into_iter().find(|r| r.seat == p)).map(|r| read_lines(&r, &s)).unwrap_or_default();
                    view! {
                        <div class="seat" class:cur=cur>
                            <span class="av">{initials}</span>
                            <span class="who">
                                <b>{name}</b>
                                <small class="cnt">{format!("{count} taş")}</small>
                                <small class="extra">
                                    {badge.map(|b| view! { <span class="open">{b}</span> })}
                                    {guess.into_iter().map(|g| view! { <span title="Tahmini eli">{g}</span> }).collect_view()}
                                </small>
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
pub(super) fn TableZone() -> impl IntoView {
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

pub(super) fn setup_table(ctx: Ctx, s: &State) -> impl IntoView + use<> {
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
pub(super) enum Mark {
    Plain,
    Group(usize),
    Lay(usize),
    Swap(usize),
    Throw,
}

pub(super) fn take_tile(rest: &mut Vec<Tile>, t: Tile) -> bool {
    match rest.iter().position(|&x| x == t) {
        Some(i) => {
            rest.remove(i);
            true
        }
        None => false,
    }
}

pub(super) fn arrange(hand: &[Tile], steps: Option<&[Step]>) -> Vec<Vec<(Tile, Mark)>> {
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

pub(super) fn split_shelves(chunks: Vec<Vec<(Tile, Mark)>>) -> [Vec<Vec<(Tile, Mark)>>; 2] {
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
pub(super) fn Rack() -> impl IntoView {
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

/// Four rows of thirteen tiles, the okey closing the first.
pub(super) fn tile_grid(cell: impl Fn(Tile) -> AnyView) -> AnyView {
    (0..4u8)
        .map(|c| {
            let tiles = (1..=13u8).map(|n| cell(id(c, n))).collect_view();
            let extra = if c == 0 { cell(JOKER) } else { view! { <span></span> }.into_any() };
            view! { {tiles} {extra} }
        })
        .collect_view()
        .into_any()
}

pub(super) fn picker_hint(s: &State) -> String {
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
pub(super) fn Picker() -> impl IntoView {
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
                    .into_any()
                };
                view! {
                    <div class="picker-head">{picker_hint(&s)}</div>
                    <div class="grid">{tile_grid(cell)}</div>
                }
            }}
        </div>
    }
}
