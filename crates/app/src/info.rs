//! The match, saved hands and the tile information windows.

use super::*;

#[component]
pub(super) fn MatchBox() -> impl IntoView {
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

pub(super) fn download(name: &str, text: &str) -> Option<()> {
    let parts = js_sys::Array::of1(&JsValue::from_str(text));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("application/json");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts).ok()?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).ok()?;
    let a: web_sys::HtmlAnchorElement = web_sys::window()?.document()?.create_element("a").ok()?.dyn_into().ok()?;
    a.set_href(&url);
    a.set_download(name);
    a.click();
    web_sys::Url::revoke_object_url(&url).ok()
}

#[component]
pub(super) fn Replays() -> impl IntoView {
    let tick = RwSignal::new(0u32);
    let note = RwSignal::new(None::<&'static str>);
    let count = move || {
        tick.get();
        let all = replay::saved();
        let done = all.iter().filter(|h| h.complete).count();
        match all.len() {
            0 => "Henüz kayıtlı el yok. Her el, puanı yazılıp yeni ele geçilince kendiliğinden kaydedilir.".to_string(),
            n if done == n => format!("{n} el kayıtlı."),
            n => format!("{n} el kayıtlı ({} tanesi puanı yazılmadan geçildi).", n - done),
        }
    };
    let file = move |_| {
        let day: String = js_sys::Date::new_0().to_iso_string().into();
        let ok = download(&format!("okey101-eller-{}.json", &day[..10]), &replay::export()).is_some();
        note.set(Some(if ok { "İndirildi." } else { "İndirilemedi; Kopyala ile dene." }));
    };
    let copy = move |_| {
        if let Some(w) = web_sys::window() {
            let _ = w.navigator().clipboard().write_text(&replay::export());
            note.set(Some("Panoya kopyalandı."));
        }
    };
    let clear = move |_| {
        let sure = web_sys::window()
            .and_then(|w| w.confirm_with_message("Bütün kayıtlı eller silinsin mi?").ok())
            .unwrap_or(false);
        if sure {
            replay::clear();
            tick.update(|t| *t += 1);
            note.set(Some("Silindi."));
        }
    };
    view! {
        <div class="stack">
            <p class="muted">{count}</p>
            <button class="btn light wide" on:click=file>"Dosya olarak indir"</button>
            <button class="btn light wide" on:click=copy>"Kopyala"</button>
            <button class="btn light danger wide" on:click=clear>"Kayıtları sil"</button>
            {move || note.get().map(|n| view! { <p class="muted">{n}</p> })}
        </div>
    }
}

#[component]
pub(super) fn Info() -> impl IntoView {
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
pub(super) fn CopyState() -> impl IntoView {
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
pub(super) fn Discards() -> impl IntoView {
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
pub(super) fn HandInfo() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    move || {
        let okey = okey_tile(&st.get());
        ctx.hand.get().map(|r: HandReport| {
            let (opened, thirds) = st.with(|s| (s.players[0].opened, s.thirds));
            let total = move |v: i32| state::count(v, thirds);
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

pub(super) fn share(p: f32) -> String {
    pct(p as f64)
}

/// The smallest and largest pair counts once the 10% least likely at each end are left out.
pub(super) fn pair_range(dist: &[f32]) -> (usize, usize) {
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

pub(super) fn pairs_text(dist: &[f32]) -> String {
    let more = |k: usize| if k == MAX_PAIRS { "+" } else { "" };
    match pair_range(dist) {
        (a, b) if a == b => format!("{a}{} çift", more(a)),
        (a, b) => format!("{a}–{b}{} çift", more(b)),
    }
}

pub(super) fn read_lines(r: &Read, s: &State) -> Vec<String> {
    match r.opened {
        Opened::No => {
            let hand = if s.rules.pairs_enabled {
                format!(
                    "açılışı {}–{} · {}",
                    s.total(r.opening[0]),
                    s.total(r.opening[2]),
                    pairs_text(&r.pairs)
                )
            } else {
                format!("açılışı {}–{}", s.total(r.opening[0]), s.total(r.opening[2]))
            };
            if no_turn_left(s, r.seat) {
                return vec![hand, "sırası gelmeyecek, açamaz".to_string()];
            }
            vec![
                hand,
                format!("sıran gelmeden açar {}", share(r.p_open_soon)),
                format!("el bitmeden açar {}", share(r.p_open_end)),
            ]
        }
        _ => vec![
            format!("elinde {}–{} puan{}", r.points[0], r.points[2], okey_share(r)),
            format!("sıran gelmeden biter {}", share(r.p_finish_soon)),
            format!("el bitmeden biter {}", share(r.p_finish_end)),
        ],
    }
}

/// The deck is empty and someone else is on turn: the hand ends before this seat plays again.
pub(super) fn no_turn_left(s: &State, seat: u8) -> bool {
    s.deck_count == 0 && s.turn != seat
}

pub(super) fn finish_odds(r: &Read) -> String {
    format!(
        "Bitirme ihtimali: sana sıra gelmeden {}, el bitene kadar {}",
        share(r.p_finish_soon),
        share(r.p_finish_end)
    )
}

pub(super) fn okey_share(r: &Read) -> String {
    if r.p_okey >= 0.05 {
        format!(" · okey {}", share(r.p_okey))
    } else {
        String::new()
    }
}

/// Bands of the best opening total: 10 points, or 3 whole pers (9 points) when totals are said in thirds.
pub(super) fn opening_bands(dist: &[f32], thirds: bool) -> Vec<(i32, i32, f32)> {
    let width = if thirds { 9 } else { 10 };
    let mut out: Vec<(i32, i32, f32)> = Vec::new();
    for (v, &x) in dist.iter().enumerate() {
        let lo = v as i32 / width * width;
        match out.last_mut() {
            Some(last) if last.0 == lo => last.2 += x,
            _ => out.push((lo, lo + width - 1, x)),
        }
    }
    out
}

pub(super) fn band_label(lo: i32, hi: i32, thirds: bool) -> String {
    if thirds {
        format!("{}–{}", lo / 3, hi / 3)
    } else {
        format!("{lo}–{hi}")
    }
}

#[component]
pub(super) fn Reads() -> impl IntoView {
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
                        let bands = opening_bands(&r.opening_dist, s.thirds)
                            .into_iter()
                            .filter(|&(_, _, x)| x >= 0.03)
                            .map(|(lo, hi, x)| {
                                let reach = hi >= r.meld_req;
                                view! {
                                    <div class="rbar" class:reach=reach>
                                        <span>{band_label(lo, hi, s.thirds)}</span>
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
                        let odds = if no_turn_left(&s, r.seat) {
                            "Deste bitti, el sırası ona gelmeden bitecek: açamaz.".to_string()
                        } else {
                            format!(
                                "Açma ihtimali: sana sıra gelmeden {}, el bitene kadar {}",
                                share(r.p_open_soon),
                                share(r.p_open_end)
                            )
                        };
                        let ready = if s.rules.pairs_enabled {
                            format!("Şu an açmaya hazır: perle {}, çiftle {}", share(r.p_melds), share(r.p_pairs))
                        } else {
                            format!("Şu an açmaya hazır: {}", share(r.p_melds))
                        };
                        let finish = finish_odds(&r);
                        view! {
                            <div class="muted">
                                {format!("En büyük açılışı: yaklaşık {} (çoğunlukla {}–{})", s.total(r.opening[1]), s.total(r.opening[0]), s.total(r.opening[2]))}
                            </div>
                            <div class="rbars">{bands}</div>
                            {pairs}
                            <div>{ready}</div>
                            <div>{odds}</div>
                            <div>{finish}</div>
                        }
                        .into_any()
                    }
                    _ => view! {
                        <div>{finish_odds(&r)}</div>
                        <div>{format!("Elinde kalan: yaklaşık {} puan (çoğunlukla {}–{}, okey hariç)", r.points[1], r.points[0], r.points[2])}</div>
                        <div>{format!("Okeyi olma ihtimali: {} (olursa ayrıca +{})", share(r.p_okey), s.rules.joker_in_hand)}</div>
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
                "Görülmemiş taşlar 200 kez dağıtılıp sayılır; rakiplerin attığı taşlara ve açmamış olmalarına uyan dağıtımlar daha çok sayılır. "
                "Herkesin açılışını destenin son turuna kadar sakladığı varsayılır; açmamış olmak ancak son turda eli hazır değil demektir. "
                "Rakibin eli, attığı taşlara bakılarak kurulur: küçük taş atan büyük taş tutar. "
                "Açar ve bitirir ihtimalleri için her dağıtım elin sonuna kadar oynanır."
            </p>
            {rows}
        })
    }
}

#[component]
pub(super) fn Unseen() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    move || {
        let s = st.get();
        let seen = s.seen();
        let okey = okey_tile(&s);
        let left = |t: Tile| 2u8.saturating_sub(seen[t as usize]);
        let total: u32 = (0..NIDS).map(|i| left(i as Tile) as u32).sum();
        let cell =
            |t: Tile| view! { <span class="ucell" class:none={left(t) == 0}>{tile_view(t, okey)}<sub>{left(t)}</sub></span> }.into_any();
        view! {
            <p class="muted">{format!("Toplam {total} taş (rakiplerin eli + deste)")}</p>
            <div class="ugrid">{tile_grid(cell)}</div>
        }
    }
}
