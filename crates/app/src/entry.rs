//! Panels for entering what happens: the deal, the opponents turns, my own moves, the end of the hand.

use super::*;

pub(super) fn setup_panel(ctx: Ctx, s: &State) -> impl IntoView + use<> {
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
        Mode::Indicator => 1,
        Mode::Starter => 2,
        _ => 3,
    };
    let gtext = match s.indicator {
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
            {step(1, now, gtext, (now > 1).then_some(Mode::Indicator))}
            {step(2, now, stext, (now > 2).then_some(Mode::Starter))}
            {step(3, now, htext, None)}
        </div>
    };
    match s.mode {
        Mode::Indicator => view! {
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

pub(super) fn standing(s: &State) -> Option<String> {
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

pub(super) fn standing_short(s: &State) -> String {
    if s.history.is_empty() && s.totals == [0; 4] {
        return String::new();
    }
    let lead = s.lead();
    let x = if s.rules.partners { lead[0] } else { *lead.iter().min().unwrap() };
    if x >= 0 { format!(" · +{x}") } else { format!(" · −{}", -x) }
}

pub(super) fn match_line(s: &State) -> String {
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

pub(super) fn over_panel(ctx: Ctx, s: &State) -> impl IntoView + use<> {
    let next = format!("Yeni el ({} / {})", (s.hand_no + 1).min(s.hands_total), s.hands_total);
    let scores = s.hand_scores();
    let mult_note = s.finisher.map(|f| {
        let fin = seat_name(s, f);
        view! {
            <div class="mults">
                <span class="muted">{format!("{fin} nasıl bitti?")}</span>
                <label class="opt"><input type="checkbox" prop:checked=s.fin_okey
                    on:change=move |ev| { let v = event_target_checked(&ev); ctx.act(|s| { s.fin_okey = v; Ok(()) }) }/>"Okey atarak"</label>
                <label class="opt"><input type="checkbox" prop:checked=s.fin_in_one
                    on:change=move |ev| { let v = event_target_checked(&ev); ctx.act(|s| { s.fin_in_one = v; Ok(()) }) }/>"Elden"</label>
            </div>
        }
    });
    let rows = (0..4u8).map(|p| {
        let pv = &s.players[p as usize];
        let finished = s.finisher.is_some_and(|f| s.rules.same_team(f, p));
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

pub(super) fn opp_draw(ctx: Ctx, s: &State) -> impl IntoView + use<> {
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

pub(super) fn opp_act(ctx: Ctx, s: &State) -> impl IntoView + use<> {
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
    let in_one = pv.opened == Opened::No && !s.opened_this_turn;
    let short = (!s.may_discard()).then(|| {
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
            {in_one.then(|| view! {
                <button class="btn light danger" on:click=move |_| ctx.act(|s| { let p = s.turn; s.finish(p, None); s.fin_in_one = true; Ok(()) })>"Elden bitti"</button>
            })}
        </div>
        {(!in_one).then(|| view! { <p class="muted">"Bitirdiyse: indirdiği ve işlediği taşları gir; son taşını atınca el kendiliğinden biter."</p> })}
        {can_tap.then(|| view! { <p class="muted">"İşlediyse ya da okey aldıysa masada o pere dokun (sarı çerçeveli)."</p> })}
    }
}

/// What the tiles picked so far amount to, or why they cannot be laid.
pub(super) fn buffer_status(s: &State, problem: &Option<String>) -> String {
    match (problem, s.buffer_shape()) {
        (None, Some(sh)) if sh.is_pair() => "Çift ✓".to_string(),
        (None, Some(sh)) => format!("{} ✓ · {}", shape_label(&sh), s.points(sh.open_value(&s.rules))),
        (Some(p), _) => p.clone(),
        _ => String::new(),
    }
}

pub(super) fn meld_entry(ctx: Ctx, s: &State) -> impl IntoView + use<> {
    let who = seat_name(s, s.turn);
    let okey = okey_tile(s);
    let problem = s.meld_problem();
    let ok = problem.is_none();
    let status = buffer_status(s, &problem);
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

pub(super) fn option_label(o: &LayOption) -> String {
    match (o.tile == JOKER, o.side) {
        (false, Side::Group) => "gruba ekle".to_string(),
        (false, side) => side_text(side).to_string(),
        (true, Side::Group) => "okeyi gruba ekle".to_string(),
        (true, side) => format!("{} ({} yerine)", side_text(side), name(o.stands_for)),
    }
}

pub(super) fn okey_fine(losers: &[u8], s: &State) -> String {
    let who: Vec<&str> = losers.iter().map(|&p| SEAT_NAMES[p as usize]).collect();
    format!("okeyi koyana {} ceza ({})", s.rules.okey_lost_penalty, who.join(", "))
}

pub(super) fn meld_menu(ctx: Ctx, s: &State, idx: usize) -> impl IntoView + use<> {
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

pub(super) fn pair_entry(ctx: Ctx, s: &State, owner: u8, okey: bool) -> impl IntoView + use<> {
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

pub(super) fn manual(ctx: Ctx, s: &State) -> impl IntoView + use<> {
    let okey = okey_tile(s);
    let n = s.buffer.len();
    let problem = s.meld_problem();
    let can_meld = problem.is_none() && n >= 2;
    let status = match n {
        0 => "İstakadan taşlara dokun.".to_string(),
        1 => "Tek taş seçili: atabilirsin. İşlemek için masadaki pere dokun.".to_string(),
        _ => buffer_status(s, &problem),
    };
    let complete = s.opening_complete();
    let single = (n == 1 && s.may_discard()).then(|| s.buffer[0]);
    let buffer = s.buffer.clone();
    let short = (!s.may_discard()).then(|| view! {
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
