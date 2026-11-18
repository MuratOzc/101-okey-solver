use crate::Ctx;
use leptos::prelude::*;
use okey_engine::rules::Rules;

type GetB = fn(&Rules) -> bool;
type SetB = fn(&mut Rules, bool);
type GetN = fn(&Rules) -> i32;
type SetN = fn(&mut Rules, i32);

macro_rules! b {
    ($label:expr, $f:ident) => {
        ($label, (|r: &Rules| r.$f) as GetB, (|r: &mut Rules, v: bool| r.$f = v) as SetB)
    };
}

macro_rules! n {
    ($label:expr, $f:ident) => {
        (
            $label,
            (|r: &Rules| r.$f as i32) as GetN,
            (|r: &mut Rules, v: i32| r.$f = v as _) as SetN,
        )
    };
}

fn bools() -> Vec<(&'static str, Vec<(&'static str, GetB, SetB)>)> {
    vec![
        (
            "Açma",
            vec![
                b!("Çiftle açma var", pairs_enabled),
                b!("Okey + tek taş = çift", joker_pairs_with_single),
                b!("İki okey = çift", two_jokers_pair),
                b!("Gösterge (dağıtımda geldiyse) herhangi bir taşla çift", indicator_pair),
                b!("12-13-1 serisi geçerli", one_after_thirteen),
            ],
        ),
        (
            "Yandan alma",
            vec![
                b!("Yandan alınan taş o tur kullanılmalı", take_must_use),
                b!("Yandan alma cezası (taş değeri × çarpan)", take_penalty_enabled),
            ],
        ),
        (
            "İşleme ve okey",
            vec![
                b!("Açtıktan sonra yeni per indirilebilir", new_melds_after_open),
                b!("Çift açan perlere işleyebilir", pair_opener_can_layoff),
                b!("Çift açan seri/grup indirebilir", pair_opener_can_meld),
                b!("Çift açana çift işlenebilir", pairs_onto_pair_opener),
                b!("Masadan okey alınabilir (açtıktan sonra)", joker_retrieve),
                b!("Gruptan okey alınca grup 4'lü kalmalı", group_joker_must_complete),
            ],
        ),
        (
            "Puan",
            vec![
                b!("Elden bitmede de açılış barajı aranır", in_one_requires_open_min),
                b!("Açıp aynı turda işleyerek bitirmek de elden sayılır", open_finish_is_in_one),
                b!("İşlek cezası açmamış oyuncuya da yazılır", playable_applies_unopened),
            ],
        ),
    ]
}

fn nums() -> Vec<(&'static str, Vec<(&'static str, GetN, SetN)>)> {
    vec![
        (
            "Dağıtım ve açma",
            vec![
                n!("Eldeki taş", hand_size),
                n!("Başlayana fazla taş", starter_extra),
                n!("Perle açma barajı", open_min),
                n!("Çiftle açma (en az çift)", pair_min),
                n!("Katlamalı: per çıtası artışı", raise_meld_by),
                n!("Katlamalı: çift çıtası artışı", raise_pair_by),
            ],
        ),
        (
            "Per ve işleme",
            vec![
                n!("Per en az taş", meld_min_len),
                n!("Yeni seri en fazla taş", max_run_len),
                n!("Bir uca turda en fazla işleme", layoff_per_end),
                n!("Çift alanına turda en fazla çift", pairs_per_turn),
                n!("12-13-1'de 1'in değeri", high_one_value),
            ],
        ),
        (
            "Puanlar",
            vec![
                n!("Bitiren", score_finish),
                n!("Açmayan", score_unopened),
                n!("Çift açanın eli çarpanı", pair_hand_mult),
                n!("Elde kalan okey başına", joker_in_hand),
                n!("Okey atıp bitirme çarpanı", mult_okey_finish),
                n!("Elden bitme çarpanı", mult_in_one),
                n!("Çiftle bitirme çarpanı", mult_pair_finish),
            ],
        ),
        (
            "Cezalar",
            vec![
                n!("İşlek taş atma", penalty_playable),
                n!("Okey atma", penalty_discard_joker),
                n!("Okeyini rakibe kaptırma", okey_lost_penalty),
                n!("Çok çiftle açana eksi yazılır (0 = yok)", big_pairs_penalty),
                n!("Bunun için en az çift", big_pairs_min),
                n!("Yandan alma çarpanı", take_penalty_mult),
            ],
        ),
    ]
}

#[component]
pub fn Basics() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    let set_rules = move |f: Box<dyn FnOnce(&mut Rules)>| {
        ctx.act(|s| {
            f(&mut s.rules);
            Ok(())
        })
    };
    view! {
        <div class="settings">
            <h3>"Oyun"</h3>
            <div class="seg">
                <button class="btn light" class:on=move || st.with(|s| !s.rules.partners)
                    on:click=move |_| set_rules(Box::new(|r| r.partners = false))>"Tekli"</button>
                <button class="btn light" class:on=move || st.with(|s| s.rules.partners)
                    on:click=move |_| set_rules(Box::new(|r| r.partners = true))>"Eşli"</button>
            </div>
            <h3>"Per sayımı"</h3>
            <div class="seg">
                <button class="btn light" class:on=move || st.with(|s| !s.thirds)
                    on:click=move |_| ctx.act(|s| { s.thirds = false; Ok(()) })>{move || st.with(|s| s.rules.open_min.to_string())}</button>
                <button class="btn light" class:on=move || st.with(|s| s.thirds)
                    on:click=move |_| ctx.act(|s| { s.thirds = true; Ok(()) })>{move || st.with(|s| crate::state::count(s.rules.open_min, true))}</button>
            </div>
            <label class="opt">
                <input type="checkbox"
                    prop:checked=move || st.with(|s| s.rules.raising)
                    on:change=move |ev| { let v = event_target_checked(&ev); set_rules(Box::new(move |r| r.raising = v)) }
                />
                "Katlamalı"
            </label>
            <label class="opt">
                <input type="checkbox"
                    prop:checked=move || st.with(|s| s.model)
                    on:change=move |ev| { let v = event_target_checked(&ev); ctx.act(move |s| { s.model = v; Ok(()) }) }
                />
                "Yapay zekâ"
            </label>
            <p class="muted">"Simülasyonlarda sonraki turlarını eğitilmiş model oynar; daha isabetli ama daha yavaş. Kurallar değiştirilmişse ve maçın son iki elinde devreye girmez."</p>
        </div>
    }
}

#[component]
pub fn Advanced() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    let set_rules = move |f: Box<dyn FnOnce(&mut Rules)>| {
        ctx.act(|s| {
            f(&mut s.rules);
            Ok(())
        })
    };
    view! {
        <details class="advanced">
            <summary>"Masa kuralları"</summary>
            <p class="muted">"Varsayılanlar senin masanın kuralları. Yalnızca farklı bir masada oynuyorsan değiştir."</p>
            {bools().into_iter().map(|(title, items)| view! {
                <h4>{title}</h4>
                {items.into_iter().map(|(label, get, set)| view! {
                    <label class="opt">
                        <input type="checkbox"
                            prop:checked=move || st.with(|s| get(&s.rules))
                            on:change=move |ev| { let v = event_target_checked(&ev); set_rules(Box::new(move |r| set(r, v))) }
                        />
                        {label}
                    </label>
                }).collect_view()}
            }).collect_view()}
            {nums().into_iter().map(|(title, items)| view! {
                <h4>{title}</h4>
                {items.into_iter().map(|(label, get, set)| view! {
                    <label class="opt num">
                        <span>{label}</span>
                        <input type="number"
                            prop:value=move || st.with(|s| get(&s.rules).to_string())
                            on:change=move |ev| {
                                if let Ok(v) = event_target_value(&ev).trim().parse::<i32>() {
                                    set_rules(Box::new(move |r| set(r, v)))
                                }
                            }
                        />
                    </label>
                }).collect_view()}
            }).collect_view()}
            <button class="btn light wide" on:click=move |_| set_rules(Box::new(|r| *r = Rules::default()))>"Varsayılan kurallara dön"</button>
        </details>
    }
}
