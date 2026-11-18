//! The advice for my turn: draw or take, the move and its steps, the alternatives.

use super::*;

pub(super) fn find(r: &Report, d: Decision) -> Option<&CandReport> {
    r.candidates.iter().find(|c| c.decision == d)
}

pub(super) fn advantage(c: &CandReport) -> String {
    let lead = -c.gap;
    if lead >= 0.0 {
        format!("Rakiplere göre: +{lead:.0} puan")
    } else {
        format!("Rakiplere göre: −{:.0} puan", -lead)
    }
}

pub(super) fn last_if_all_draw(deck: u8, i_draw: bool) -> Option<u8> {
    if deck == 0 {
        return None;
    }
    let first = if i_draw { 0 } else { 1 };
    Some(((first + deck as u32 - 1) % 4) as u8)
}

pub(super) fn last_lines(s: &State, c: &CandReport) -> Vec<String> {
    let p = c.p_last_draw;
    let before = format!("El ondan önce biter: {}", pct(1.0 - p.iter().sum::<f64>()));
    if s.rules.partners {
        return vec![
            format!(
                "Son taşı sizin takım çeker: {} (sen {}, eşin {})",
                pct(p[0] + p[2]),
                pct(p[0]),
                pct(p[2])
            ),
            format!("Karşı takım çeker: {}", pct(p[1] + p[3])),
            before,
        ];
    }
    let mut order: Vec<usize> = (0..4).collect();
    order.sort_by(|&a, &b| p[b].total_cmp(&p[a]));
    let mut lines: Vec<String> = order
        .iter()
        .filter(|&&i| p[i] >= 0.005)
        .map(|&i| {
            let who = if i == 0 { "Sen".to_string() } else { seat_name(s, i as u8) };
            format!("Son taşı {who} çeker: {}", pct(p[i]))
        })
        .collect();
    lines.push(before);
    lines
}

#[component]
pub(super) fn MyDraw() -> impl IntoView {
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
                None => format!("Öbür seçenekten ortalama {:.1} puan daha iyi", (d.mean - t.mean).abs()),
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
                    last_lines(&s, c)
                        .into_iter()
                        .map(|l| view! { <span class="muted">{l}</span> })
                        .collect_view()
                });
                view! {
                    <div class="lt-row">
                        <span>{label}": herkes desteden çekerse son taş "<b>{last.unwrap_or_default()}</b></span>
                        {sim}
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

/// A turn in a few words, from its steps.
pub(super) fn turn_title(steps: &[Step], s: &State) -> String {
    let unopened = s.players[0].opened == Opened::No;
    let (mut value, mut melds, mut pairs, mut gifts, mut swaps) = (0, 0, 0, 0, false);
    let mut laid: Vec<Tile> = Vec::new();
    let mut throw = None;
    for step in steps {
        match step {
            Step::Meld { shape, owner: 0 } if shape.is_pair() => pairs += 1,
            Step::Meld { shape, owner: 0 } => {
                melds += 1;
                value += shape.open_value(&s.rules);
            }
            Step::Meld { .. } => gifts += 1,
            Step::Layoff { tile, .. } => laid.push(*tile),
            Step::Swap { .. } => swaps = true,
            Step::Discard { tile, finish } => throw = Some((*tile, *finish)),
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
    if let Some((t, finish)) = throw {
        parts.push(format!("{} {}", name(t), if finish { "atıp bitir" } else { "at" }));
    }
    parts.join(" · ")
}

pub(super) fn step_view(step: &Step, n: usize, okey: Option<Tile>, s: &State) -> impl IntoView + use<> {
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
pub(super) fn Plan() -> impl IntoView {
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
        let title = turn_title(&steps, &s);
        let stats = cand.as_ref().map(|c| {
            let mut lines = Vec::new();
            if let Some(w) = c.p_win {
                lines.push(format!("Maçı kazanma ihtimalin: {}", chance(w)));
            }
            lines.push(advantage(c));
            if c.whole >= ENOUGH_ENDINGS {
                if s.players[0].opened == Opened::No {
                    lines.push(format!("Açma ihtimalin: {}", pct(c.p_open)));
                }
                lines.push(format!("Bitirme ihtimalin: {}", pct(c.p_finish)));
            }
            lines.push(format!("Taşını sağdaki alır: {}", pct(c.p_next_takes)));
            lines.into_iter().map(|l| view! { <span class="line">{l}</span> }).collect_view()
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

pub(super) fn alternatives(ctx: Ctx, report: &Report, s: &State) -> impl IntoView + use<> {
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
    let view = s.view();
    let mut seen: Vec<String> = Vec::new();
    let labels: Vec<String> = rows
        .iter()
        .map(|c| {
            let label = match (&c.mv, &view) {
                (Some(mv), Some(v)) => turn_title(&describe_move(v, mv), s),
                _ => c.label.clone(),
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
                    _ if c.alive => format!("önerilenden {behind:.1} puan kötü"),
                    _ => format!("önerilenden {behind:.0} puan kötü"),
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
