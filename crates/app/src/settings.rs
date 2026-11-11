use crate::Ctx;
use leptos::prelude::*;
use okey_engine::rules::{Rules, TakenJokerPenalty};

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
        ("Genel", vec![b!("Eşli oyun (karşılıklı takım)", partners)]),
        (
            "Açma",
            vec![
                b!("Çiftle açma var", pairs_enabled),
                b!("Katlamalı: her açılışta çıta yükselir", katlamali),
                b!("Okey + tek taş = çift", joker_pairs_with_single),
                b!("İki okey = çift", two_jokers_pair),
                b!("Gösterge (dağıtımda geldiyse) herhangi bir taşla çift", gosterge_pair),
                b!("12-13-1 serisi geçerli", ace_after_king),
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
                b!("Elden bitmede de açılış barajı aranır", elden_requires_open_min),
                b!("Açıp aynı turda işleyerek bitirmek de elden sayılır", open_finish_is_elden),
                b!("İşlek cezası açmamış oyuncuya da yazılır", islek_applies_unopened),
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
                n!("12-13-1'de 1'in değeri", ace_high_value),
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
                n!("Elden bitme çarpanı", mult_elden),
                n!("Çiftle bitirme çarpanı", mult_pair_finish),
            ],
        ),
        (
            "Cezalar",
            vec![
                n!("İşlek taş atma", penalty_islek),
                n!("Okey atma", penalty_discard_joker),
                n!("Okeyini rakibe kaptırma", okey_lost_penalty),
                n!("Yandan alma çarpanı", take_penalty_mult),
            ],
        ),
    ]
}

#[component]
pub fn Settings() -> impl IntoView {
    let ctx = expect_context::<Ctx>();
    let st = ctx.st;
    let set_rules = move |f: Box<dyn FnOnce(&mut Rules)>| {
        ctx.act(|s| {
            f(&mut s.rules);
            Ok(())
        })
    };
    view! {
        <section class="card settings">
            <div class="row">
                <h2>"Kurallar"</h2>
                <button on:click=move |_| ctx.show_settings.set(false)>"Kapat"</button>
                <button on:click=move |_| set_rules(Box::new(|r| *r = Rules::default()))>"Varsayılana dön"</button>
            </div>
            {bools().into_iter().map(|(title, items)| view! {
                <h3>{title}</h3>
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
            <label class="opt">
                <input type="checkbox"
                    prop:checked=move || st.with(|s| s.rules.take_joker_penalty == TakenJokerPenalty::PlacedValue)
                    on:change=move |ev| {
                        let v = event_target_checked(&ev);
                        set_rules(Box::new(move |r| r.take_joker_penalty = if v { TakenJokerPenalty::PlacedValue } else { TakenJokerPenalty::None }))
                    }
                />
                "Yandan alınan okeye de ceza"
            </label>
            {nums().into_iter().map(|(title, items)| view! {
                <h3>{title}</h3>
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
        </section>
    }
}
