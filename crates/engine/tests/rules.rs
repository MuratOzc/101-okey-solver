//! One test per table rule the user specified. A failing test here means the engine
//! is recommending or simulating something that is illegal at the user's table.

use okey_engine::analysis::*;
use okey_engine::game::*;
use okey_engine::meld::*;
use okey_engine::policy::Policy;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use okey_engine::table::*;
use okey_engine::tile::*;

fn t(s: &str) -> Tile {
    parse(s).unwrap_or_else(|| panic!("bad tile {s}"))
}

fn hand(s: &str) -> Counts {
    counts_from(&s.split_whitespace().map(t).collect::<Vec<_>>())
}

fn game_with(rules: Rules, hands: [&str; 4]) -> Game {
    let mut seen = [0; NIDS];
    seen[t("S7") as usize] = 1;
    Game {
        indicator: t("S7"),
        players: hands.map(|h| Player {
            hand: hand(h),
            opened: Opened::No,
            has_indicator: false,
            penalty: 0,
        }),
        deck: vec![],
        table: Table::default(),
        top: None,
        current: 0,
        meld_req: rules.open_min,
        pair_req: rules.pair_min,
        seen,
        piles: [[0; NIDS]; 4],
        last: [None; 4],
        last_draw: None,
        log: None,
        policy: [Default::default(); 4],
        stakes: None,
        rules,
    }
}

fn game(hands: [&str; 4]) -> Game {
    game_with(Rules::default(), hands)
}

fn put(g: &mut Game, owner: u8, tiles: &str) -> usize {
    let tiles: Vec<Tile> = tiles.split_whitespace().map(t).collect();
    let shape = classify(&tiles, &g.rules).expect("valid table meld");
    put_shape(g, owner, shape)
}

fn put_shape(g: &mut Game, owner: u8, shape: Shape) -> usize {
    for tile in shape.tiles() {
        g.seen[tile as usize] += 1;
    }
    g.table.melds.push(TableMeld::new(owner, shape));
    if g.players[owner as usize].opened == Opened::No {
        g.players[owner as usize].opened = if shape.is_pair() { Opened::Pairs } else { Opened::Melds };
    }
    g.table.melds.len() - 1
}

fn set_top(g: &mut Game, tile: &str) {
    g.top = Some(t(tile));
    g.seen[t(tile) as usize] += 1;
}

fn sv(g: &Game) -> Solver {
    Solver::new(&g.rules)
}

const OPEN_101: &str = "K9 K10 K11 K12 K13 M10 M11 M12 M13";
const OPEN_100: &str = "K9 K10 K11 K12 K13 M11 M12 M13 S2 S3 S4";

// ---------- opening ----------

#[test]
fn melds_open_at_101() {
    let mut g = game([&format!("{OPEN_100} Si8"), "", "", ""]);
    assert!(!g.try_open_melds(&mut sv(&g), 0, None), "100 points must not open");
    assert_eq!(g.players[0].opened, Opened::No);

    let mut g = game([&format!("{OPEN_101} Si8 Si2"), "", "", ""]);
    assert!(g.try_open_melds(&mut sv(&g), 0, None), "exactly 101 opens");
    assert_eq!(g.players[0].opened, Opened::Melds);
    assert_eq!(g.table.melds.len(), 2);
}

#[test]
fn pairs_open_at_five() {
    let mut g = game(["K1 K1 M2 M2 S3 S3 Si4 Si4 K6 M9", "", "", ""]);
    assert!(!g.try_open_pairs(0, None), "four pairs must not open");
    let mut g = game(["K1 K1 M2 M2 S3 S3 Si4 Si4 K6 K6 M9", "", "", ""]);
    assert!(g.try_open_pairs(0, None));
    assert_eq!(g.players[0].opened, Opened::Pairs);
}

#[test]
fn okey_pairs_but_two_okeys_dont() {
    let mut g = game(["K1 K1 M2 M2 S3 S3 Si4 Si4 K6 OK M9", "", "", ""]);
    assert!(g.try_open_pairs(0, None), "okey + K6 is the fifth pair");
    let mut g = game(["K1 K1 M2 M2 S3 S3 Si4 Si4 OK OK", "", "", ""]);
    assert!(!g.try_open_pairs(0, None), "two okeys do not make the fifth pair");
}

#[test]
fn dealt_indicator_pairs_with_any() {
    let h = "K1 K1 M2 M2 S3 S3 Si4 Si4 S7 K11 M9";
    let mut g = game([h, "", "", ""]);
    assert!(!g.try_open_pairs(0, None), "without the gösterge right it is only four pairs");
    let mut g = game([h, "", "", ""]);
    g.players[0].has_indicator = true;
    assert!(g.try_open_pairs(0, None), "gösterge (S7) + K11 counts as the fifth pair");
}

#[test]
fn raising_thresholds_are_separate() {
    let rules = Rules {
        raising: true,
        ..Rules::default()
    };
    let mut g = game_with(
        rules,
        [
            &format!("{OPEN_101} Si8"),
            "S9 S10 S11 S12 S13 Si10 Si11 Si12 Si13 K1",
            "K1 K1 M2 M2 S3 S3 Si4 Si4 K6 K6 M9",
            "",
        ],
    );
    assert!(g.try_open_melds(&mut sv(&g), 0, None));
    assert_eq!(g.meld_req, 102, "next meld opener needs one more than 101");
    assert_eq!(g.pair_req, 5, "a meld opening does not touch the pair threshold");
    assert!(!g.try_open_melds(&mut sv(&g), 1, None), "101 no longer opens");
    assert!(g.try_open_pairs(2, None));
    assert_eq!(g.pair_req, 6, "next pair opener needs six pairs");
    assert_eq!(g.meld_req, 102);
}

#[test]
fn fixed_thresholds_without_raising() {
    let mut g = game([&format!("{OPEN_101} Si8"), "S9 S10 S11 S12 S13 Si10 Si11 Si12 Si13 K1", "", ""]);
    assert!(g.try_open_melds(&mut sv(&g), 0, None));
    assert_eq!(g.meld_req, 101);
    assert!(g.try_open_melds(&mut sv(&g), 1, None));
}

// ---------- nothing on the table before opening ----------

#[test]
fn unopened_cannot_touch_table() {
    let mut g = game(["M4 M8 K1 K2 K3 Si9", "", "", ""]);
    put_shape(
        &mut g,
        1,
        Shape::Run {
            color: 2,
            lo: 4,
            hi: 7,
            jokers: 1 << 4,
        },
    );
    let before = g.clone();
    for choice in [OpenChoice::Finish, OpenChoice::OPEN, OpenChoice::PAIRS, OpenChoice::LAY] {
        let mut g2 = g.clone();
        assert!(
            g2.apply_choice(&mut sv(&g), 0, choice, None).is_err(),
            "{choice:?} must be illegal before opening"
        );
    }
    assert_eq!(g.table.melds[0].shape, before.table.melds[0].shape);
    assert!(!g.in_one_possible(&mut sv(&g), 0, None));
}

#[test]
fn laying_off_is_not_opening() {
    // K1-K2-K3 is only 6 points: M8 may not go onto the table run just because the hand would empty.
    let mut g = game(["M8 K1 K2 K3 Si9", "", "", ""]);
    put(&mut g, 1, "M5 M6 M7");
    assert!(!g.in_one_possible(&mut sv(&g), 0, None));
    assert!(g.clone().apply_choice(&mut sv(&g), 0, OpenChoice::Finish, None).is_err());
}

fn steps_respect_opening(view: &View, seed: u64) {
    let (meld_req, pair_req) = view.requirements();
    let unopened = view.players[0].opened == Opened::No;
    let mut solver = Solver::new(&view.rules);
    for d in candidates(view, &mut solver, &Options::default()) {
        let (mut value, mut pairs) = (0, 0u8);
        for step in describe(view, d) {
            match step {
                Step::Meld { shape, owner: 0 } if shape.is_pair() => pairs += 1,
                Step::Meld { shape, owner: 0 } => value += shape.open_value(&view.rules),
                Step::Discard { .. } => {}
                other => {
                    if unopened {
                        assert!(
                            value >= meld_req || pairs >= pair_req,
                            "seed {seed}: {d:?} touches the table ({other:?}) after laying only {value} points / {pairs} pairs"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn advice_waits_for_opening() {
    for raising in [false, true] {
        let rules = Rules {
            raising,
            ..Rules::default()
        };
        let mut solver = Solver::new(&rules);
        let mut checked = 0;
        for seed in 0..400u64 {
            let mut g = Game::deal(&rules, &mut Rng::new(seed), 1);
            let mut ended = g.starter_turn(&mut solver).is_some();
            let mut my_turns = 0;
            while !ended && my_turns < 4 {
                if g.current == 0 {
                    my_turns += 1;
                    g.table.new_turn();
                    let Some(tile) = g.deck.pop() else { break };
                    g.players[0].hand[tile as usize] += 1;
                    let view = public_view(&g, Phase::AfterDraw, None);
                    steps_respect_opening(&view, seed);
                    checked += 1;
                    ended = g.bot_act(&mut solver, 0, None).is_some();
                    g.current = 1;
                } else {
                    ended = g.bot_turn(&mut solver).is_some();
                }
            }
        }
        assert!(checked > 500, "only {checked} positions checked");
    }
}

// ---------- in-one bitme ----------

const ALL_MELDS: &str = "K1 K2 K3 M1 M2 M3 S1 S2 S3 Si1 Si2 Si3 K5 M5 S5 K7 M7 Si7 K9 M9 S9";

#[test]
fn in_one_needs_only_own_melds() {
    let mut g = game([&format!("{ALL_MELDS} Si13"), "K5", "K5", "K5"]);
    let out = g.apply_choice(&mut sv(&g), 0, OpenChoice::Finish, None).unwrap().unwrap();
    let Outcome::Finished(f) = out else { panic!("should finish") };
    assert!(f.in_one && !f.okey && !f.pairs);
    assert_eq!(f.last, t("Si13"));
    assert_eq!(g.scores(&out), [-202, 202, 202, 202], "elden doubles everything");
}

#[test]
fn open_and_lay_off_is_normal_finish() {
    let mut g = game([&format!("{OPEN_101} M8 Si2"), "K5", "", ""]);
    put(&mut g, 1, "M5 M6 M7");
    assert!(!g.in_one_possible(&mut sv(&g), 0, None));
    let out = g.apply_choice(&mut sv(&g), 0, OpenChoice::OPEN, None).unwrap().unwrap();
    let Outcome::Finished(f) = out else { panic!("should finish") };
    assert!(!f.in_one, "opening and finishing by laying off is not elden");
    assert_eq!(g.scores(&out)[0], -101);
}

#[test]
fn okey_finish_doubles() {
    let mut g = game([&format!("{ALL_MELDS} OK"), "K5", "", ""]);
    let out = g.apply_choice(&mut sv(&g), 0, OpenChoice::Finish, None).unwrap().unwrap();
    let Outcome::Finished(f) = out else { panic!("should finish") };
    assert!(f.okey && f.in_one);
    assert_eq!(g.scores(&out)[..2], [-404, 404], "okey × elden multiply to four");
}

// ---------- scoring ----------

fn finished(player: u8, okey: bool, in_one: bool, pairs: bool) -> Outcome {
    Outcome::Finished(FinishInfo {
        player,
        okey,
        in_one,
        pairs,
        last: 0,
    })
}

#[test]
fn end_of_hand_scores() {
    let mut g = game(["", "K5 M9", "K5 M9", "K5 M9"]);
    g.players[2].opened = Opened::Melds;
    g.players[3].opened = Opened::Pairs;
    assert_eq!(g.scores(&finished(0, false, false, false)), [-101, 101, 14, 28]);
    assert_eq!(
        g.scores(&finished(0, true, false, false)),
        [-202, 202, 28, 56],
        "okey finish doubles"
    );
    assert_eq!(
        g.scores(&finished(0, false, false, true)),
        [-202, 202, 28, 56],
        "pair finish doubles"
    );
    assert_eq!(g.scores(&finished(0, true, true, false)), [-404, 404, 56, 112]);
}

#[test]
fn okey_in_hand_costs_101() {
    let mut g = game(["", "K5 OK", "K5 OK", ""]);
    g.players[2].opened = Opened::Melds;
    let s = g.scores(&finished(0, false, false, false));
    assert_eq!(s[1], 202, "unopened 101 + okey 101");
    assert_eq!(s[2], 106, "5 + okey 101");
}

#[test]
fn penalties_not_multiplied() {
    let mut g = game(["", "K5", "", ""]);
    g.players[1].penalty = 101;
    assert_eq!(g.scores(&finished(0, true, false, false))[1], 202 + 101);
}

#[test]
fn deck_out_scores() {
    let mut g = game(["K5 M9", "K5", "", ""]);
    g.players[0].opened = Opened::Melds;
    assert_eq!(g.scores(&Outcome::DeckOut)[..2], [14, 101]);
}

#[test]
fn partner_finish_minus_202() {
    let rules = Rules {
        partners: true,
        ..Rules::default()
    };
    let g = game_with(rules, ["", "K5 M9", "K13 K12 K11 OK", "K5"]);
    let s = g.scores(&finished(0, false, false, false));
    assert_eq!(s[0] + s[2], -202, "my partner's hand does not count when I finish");
    assert_eq!(s[1] + s[3], 202, "the other team adds both penalties");
}

// ---------- discards ----------

#[test]
fn discard_penalties() {
    let mut g = game(["M8 M4 K1 OK", "", "", ""]);
    put(&mut g, 1, "M5 M6 M7");
    assert_eq!(g.discard_penalty(0, t("M8")), 101, "işlek tile, even for an unopened player");
    assert_eq!(g.discard_penalty(0, t("M4")), 101);
    assert_eq!(g.discard_penalty(0, t("K1")), 0);
    assert_eq!(g.discard_penalty(0, JOKER), 101, "discarding okey");
    g.players[0].hand = hand("OK");
    assert_eq!(g.discard_penalty(0, JOKER), 0, "the finishing discard is free");
}

#[test]
fn playable_after_two_layoffs() {
    let mut g = game(["M11 K1", "", "", ""]);
    let i = put(&mut g, 1, "M6 M7 M8");
    assert!(g.table.lay_off_on(i, t("M9"), None, &g.rules.clone()).is_some());
    assert!(g.table.lay_off_on(i, t("M10"), None, &g.rules.clone()).is_some());
    assert_eq!(g.discard_penalty(0, t("M11")), 101);
}

// ---------- taking the side tile ----------

#[test]
fn side_tile_only_if_used() {
    let mut g = game([&format!("{OPEN_100} K1"), "", "", ""]);
    set_top(&mut g, "Si8");
    assert!(!g.can_use_taken(&mut sv(&g), 0, t("Si8")), "cannot open with it, so cannot take it");

    let mut g = game(["K9 K10 K11 K12 K13 M11 M12 M13 Si8 Si2", "", "", ""]);
    set_top(&mut g, "M10");
    assert!(g.can_use_taken(&mut sv(&g), 0, t("M10")), "M10 completes a 101 opening");

    let mut g = game(["K1 Si9 S4", "", "", ""]);
    put(&mut g, 1, "M5 M6 M7");
    g.players[0].opened = Opened::Melds;
    assert!(
        g.can_use_taken(&mut sv(&g), 0, t("M8")),
        "an opened player may take a tile to lay off"
    );
    assert!(!g.can_use_taken(&mut sv(&g), 0, t("K12")));
}

#[test]
fn taken_tile_must_be_in_opening() {
    // The hand already opens without S2; S2 itself fits nothing, so it may not be taken.
    let g = game([&format!("{OPEN_101} Si8"), "", "", ""]);
    assert!(!g.can_use_taken(&mut sv(&g), 0, t("S2")));
}

#[test]
fn take_penalty_ten_times() {
    let rules = Rules {
        take_penalty_enabled: true,
        ..Rules::default()
    };
    let mut g = game_with(rules, ["K1", "", "", ""]);
    set_top(&mut g, "K4");
    g.take_top(0);
    assert_eq!(g.players[0].penalty, 40);

    let mut g = game(["K1", "", "", ""]);
    set_top(&mut g, "K4");
    g.take_top(0);
    assert_eq!(g.players[0].penalty, 0, "off by default");
}

#[test]
fn taken_okey_costs_nothing() {
    let rules = Rules {
        take_penalty_enabled: true,
        ..Rules::default()
    };
    let mut g = game_with(rules, ["K1", "", "", ""]);
    set_top(&mut g, "OK");
    g.take_top(0);
    assert_eq!(g.players[0].penalty, 0, "throwing it already cost the thrower");
}

// ---------- after opening ----------

#[test]
fn pair_opener_limits() {
    let mut g = game(["K1 K1 M2 M2 S3 S3 Si4 Si4 K6 K6 M7 M8 M9 S11 Si12", "", "", ""]);
    put(&mut g, 1, "M4 M5 M6");
    let out = g.apply_choice(&mut sv(&g), 0, OpenChoice::PAIRS, None).unwrap();
    assert!(out.is_none());
    assert!(
        g.table.melds.iter().filter(|m| m.owner == 0).all(|m| m.shape.is_pair()),
        "a pair opener's own melds are pairs only"
    );
    assert_eq!(
        g.table.melds[0].shape,
        Shape::Run {
            color: 2,
            lo: 4,
            hi: 8,
            jokers: 0
        },
        "M7 and M8 were laid off, two per end"
    );
    assert_eq!(g.players[0].hand[t("M9") as usize], 1, "M9 would be the third tile on that end");
}

#[test]
fn three_pairs_per_turn() {
    let mut g = game(["K1 K1 M2 M2 S3 S3 Si4 Si4 K9", "", "", ""]);
    put(&mut g, 0, "K13 K13");
    assert!(g.lay(&mut sv(&g), 0, None, false).unwrap().is_none());
    assert_eq!(g.table.pairs_added[0], 3);
    assert_eq!(total(&g.players[0].hand), 3, "the fourth pair waits for next turn");
    g.table.new_turn();
    assert!(
        g.lay(&mut sv(&g), 0, None, false).unwrap().is_some(),
        "next turn the last pair goes down and K9 finishes"
    );
}

#[test]
fn meld_opener_gives_pairs() {
    let mut g = game(["K1 K1 M2 M2 S3 S3 Si4 Si4 K6 K6 M9 M9 S11 S11 K9", "", "", ""]);
    g.players[0].opened = Opened::Melds;
    put(&mut g, 1, "K13 K13");
    put(&mut g, 2, "M13 M13");
    assert!(g.lay(&mut sv(&g), 0, None, false).unwrap().is_none());
    assert_eq!(g.table.pairs_added, [0, 3, 3, 0], "three pairs to each pair opener");
    assert_eq!(total(&g.players[0].hand), 3);
}

#[test]
fn meld_opener_has_no_pairs() {
    let mut g = game(["K1 K1 M2 M2 S3 S3 Si4 Si4 K6 K6 K9", "", "", ""]);
    g.players[0].opened = Opened::Melds;
    assert!(g.clone().apply_choice(&mut sv(&g), 0, OpenChoice::PAIRS, None).is_err());
    assert!(g.lay(&mut sv(&g), 0, None, false).unwrap().is_none());
    assert!(g.table.melds.is_empty(), "nobody opened with pairs, so pairs stay in hand");
}

#[test]
fn new_runs_three_to_five() {
    let mut g = game(["M1 M2 M3 M4 M5 M6 M7 K9", "", "", ""]);
    g.players[0].opened = Opened::Melds;
    let _ = g.lay(&mut sv(&g), 0, None, false);
    assert!(
        g.table.melds.iter().all(|m| (3..=5).contains(&m.shape.len())),
        "{:?}",
        g.table.melds
    );
    assert_eq!(g.table.melds.len(), 2);
}

#[test]
fn tile_kept_to_discard() {
    let mut g = game(["M8 M9", "", "", ""]);
    g.players[0].opened = Opened::Melds;
    put(&mut g, 1, "M5 M6 M7");
    let out = g.lay(&mut sv(&g), 0, None, false).unwrap();
    assert!(matches!(out, Some(Outcome::Finished(_))));
    assert_eq!(
        g.table.melds[0].shape.len(),
        4,
        "one tile is laid off, the other is the finishing discard"
    );
}

// ---------- okey on the table ----------

#[test]
fn losing_okey_costs_101() {
    let mut g = game(["M4 K1 S9 Si3", "", "", ""]);
    g.players[0].opened = Opened::Melds;
    put_shape(
        &mut g,
        0,
        Shape::Run {
            color: 2,
            lo: 4,
            hi: 7,
            jokers: 1 << 4,
        },
    );
    let slot = g.table.joker_slots(&g.rules).remove(0);
    g.do_swap(0, &slot);
    assert_eq!(g.players[0].penalty, 0);
    assert_eq!(g.players[0].hand[JOKER as usize], 1);

    let mut g = game(["M4 K1 S9 Si3", "", "", ""]);
    g.players[0].opened = Opened::Melds;
    put_shape(
        &mut g,
        1,
        Shape::Run {
            color: 2,
            lo: 4,
            hi: 7,
            jokers: 1 << 4,
        },
    );
    let slot = g.table.joker_slots(&g.rules).remove(0);
    g.do_swap(0, &slot);
    assert_eq!(g.players[0].penalty, 0, "the taker pays nothing");
    assert_eq!(g.players[1].penalty, 101, "the player whose okey was taken pays");
    assert_eq!(g.players[0].hand[JOKER as usize], 1);
}

#[test]
fn opened_take_exposed_okey() {
    let mut g = game(["M4 K1 S9 Si3", "", "", ""]);
    g.players[0].opened = Opened::Melds;
    put_shape(
        &mut g,
        1,
        Shape::Run {
            color: 2,
            lo: 4,
            hi: 7,
            jokers: 1 << 4,
        },
    );
    let _ = g.lay(&mut sv(&g), 0, None, false);
    let Shape::Run { jokers, .. } = g.table.melds[0].shape else {
        panic!("still a run")
    };
    assert_eq!(jokers & (1 << 4), 0, "M4 now sits where the okey was");
    assert_eq!(g.players[1].penalty, 101);
}

#[test]
fn partner_okey_is_free() {
    let rules = Rules {
        partners: true,
        ..Rules::default()
    };
    let mut g = game_with(rules, ["M4 K1", "", "", ""]);
    g.players[0].opened = Opened::Melds;
    put_shape(
        &mut g,
        2,
        Shape::Run {
            color: 2,
            lo: 4,
            hi: 7,
            jokers: 1 << 4,
        },
    );
    let slot = g.table.joker_slots(&g.rules).remove(0);
    g.do_swap(0, &slot);
    assert_eq!(g.players[0].penalty + g.players[2].penalty, 0);
}

#[test]
fn group_okey_needs_full_group() {
    let r = Rules::default();
    let three = Table {
        melds: vec![TableMeld::new(1, classify(&[t("S8"), t("M8"), JOKER], &r).unwrap())],
        pairs_added: [0; 4],
    };
    let slots = three.joker_slots(&r);
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].tiles.len(), 2, "sarı 8 + mavi 8 + okey needs both kırmızı 8 and siyah 8");

    let four = Table {
        melds: vec![TableMeld::new(1, classify(&[t("S8"), t("M8"), JOKER, JOKER], &r).unwrap())],
        pairs_added: [0; 4],
    };
    assert!(
        four.joker_slots(&r).iter().any(|s| s.tiles.len() == 1 && s.jokers == 1),
        "with two okeys one color frees one okey"
    );
}

// ---------- tiles ----------

#[test]
fn one_low_and_distinct_colors() {
    let r = Rules::default();
    assert!(classify(&[t("K12"), t("K13"), t("K1")], &r).is_none(), "12-13-1 is not a run");
    assert!(classify(&[JOKER, JOKER], &r).is_none(), "two okeys are not a pair");
    assert!(classify(&[t("K5"), JOKER], &r).is_some_and(|s| s.is_pair()));
    assert!(classify(&[t("K1"), t("K2"), t("K3")], &r).is_some());
    assert!(
        classify(&[t("K5"), t("K5"), t("M5")], &r).is_none(),
        "the same color twice is not a group"
    );
    assert!(classify(&[t("K5"), t("S5"), t("M5"), t("Si5")], &r).is_some());
}

#[test]
fn okey_follows_indicator() {
    assert_eq!(okey_of(t("S7")), t("S8"));
    assert_eq!(okey_of(t("K13")), t("K1"));
    let r = Rules::default();
    assert_eq!(classify(&[t("K10"), JOKER, t("K12")], &r).unwrap().open_value(&r), 33);
    assert_eq!(classify(&[t("K10"), t("M10"), JOKER], &r).unwrap().open_value(&r), 30);
}

#[test]
fn starter_gets_22() {
    let rules = Rules::default();
    let g = Game::deal(&rules, &mut Rng::new(5), 2);
    assert_eq!(total(&g.players[2].hand), 22);
    assert!([0, 1, 3].iter().all(|&p| total(&g.players[p].hand) == 21));
    assert_eq!(g.deck.len(), 20);
}

#[test]
fn okey_kept_out_of_runs() {
    let r = Rules::default();
    let mut tb = Table::default();
    tb.melds
        .push(TableMeld::new(1, classify(&[t("M5"), t("M6"), t("M7")], &r).unwrap()));
    assert!(tb.clone().lay_off(JOKER, &r, true).is_some());
    assert!(
        tb.lay_off(JOKER, &r, false).is_none(),
        "no group on the table, so a safe okey stays in hand"
    );
    tb.melds
        .push(TableMeld::new(1, classify(&[t("K9"), t("M9"), t("S9")], &r).unwrap()));
    let (i, _) = tb.lay_off(JOKER, &r, true).unwrap();
    assert_eq!(i, 1, "an okey prefers the group even when runs are allowed");
}

#[test]
fn safe_opening_keeps_okey_out() {
    let mut g = game(["K10 K11 OK K13 M10 M11 M12 M13 S9 S10 S11 Si2", "", "", ""]);
    let ok = g.try_open_melds_style(&mut sv(&g), 0, None, okey_engine::policy::OpenStyle::SafeOkey);
    if ok {
        assert!(
            g.table
                .melds
                .iter()
                .all(|m| !matches!(m.shape, Shape::Run { jokers, .. } if jokers != 0))
        );
    }
    let mut g2 = game(["K10 K11 OK K13 M10 M11 M12 M13 S9 S10 S11 Si2", "", "", ""]);
    assert!(g2.try_open_melds(&mut sv(&g2), 0, None), "with the okey in the run this hand opens");
}

// ---------- exhaustive move generator ----------

use okey_engine::moves::{Act, Move, generate};

fn mid_game_positions(rules: &Rules, seeds: std::ops::Range<u64>, mut check: impl FnMut(&Game, u64)) -> usize {
    let mut solver = Solver::new(rules);
    let mut checked = 0;
    for seed in seeds {
        let mut g = Game::deal(rules, &mut Rng::new(seed), 1);
        let mut ended = g.starter_turn(&mut solver).is_some();
        let mut my_turns = 0;
        while !ended && my_turns < 4 {
            if g.current == 0 {
                my_turns += 1;
                g.table.new_turn();
                let Some(tile) = g.deck.pop() else { break };
                g.players[0].hand[tile as usize] += 1;
                check(&g, seed);
                checked += 1;
                ended = g.bot_act(&mut solver, 0, None).is_some();
                g.current = 1;
            } else {
                ended = g.bot_turn(&mut solver).is_some();
            }
        }
    }
    checked
}

#[test]
fn generated_moves_are_legal() {
    for raising in [false, true] {
        let rules = Rules {
            raising,
            ..Rules::default()
        };
        let checked = mid_game_positions(&rules, 0..250, |g, seed| {
            let unopened = g.players[0].opened == Opened::No;
            for acts in generate(g, 0, None, 5000).positions {
                let (mut value, mut pairs) = (0, 0u8);
                for act in &acts {
                    match act {
                        Act::Meld(s) if s.is_pair() => pairs += 1,
                        Act::Meld(s) => value += s.open_value(&rules),
                        other => assert!(
                            !unopened || value >= g.meld_req || pairs >= g.pair_req,
                            "seed {seed}: {other:?} before the opening is complete ({value} points, {pairs} pairs)"
                        ),
                    }
                }
                let mut after = g.clone();
                after.play_acts(0, &acts, None).expect("generated steps replay cleanly");
                let hand = after.players[0].hand;
                for t in (0..NIDS as Tile).filter(|&t| hand[t as usize] > 0) {
                    let mv = Move {
                        acts: acts.clone(),
                        discard: t,
                    };
                    let mut g2 = g.clone();
                    assert!(
                        g2.play_move(0, &mv, None).is_ok(),
                        "seed {seed}: {mv:?} was generated but is illegal"
                    );
                    assert_eq!(g2.tile_count(), g.tile_count(), "seed {seed}: tiles appeared or vanished");
                }
            }
        });
        assert!(checked > 300, "only {checked} positions checked");
    }
}

fn layoff_tiles(acts: &[Act]) -> Vec<Tile> {
    let mut v: Vec<Tile> = acts
        .iter()
        .filter_map(|a| if let Act::Layoff { tile, .. } = a { Some(*tile) } else { None })
        .collect();
    v.sort();
    v
}

#[test]
fn generator_finds_layoffs() {
    let mut g = game(["M8 M9 K1", "", "", ""]);
    g.players[0].opened = Opened::Melds;
    put(&mut g, 1, "M5 M6 M7");
    let mut found: Vec<Vec<Tile>> = generate(&g, 0, None, 1000).positions.iter().map(|a| layoff_tiles(a)).collect();
    found.sort();
    assert_eq!(
        found,
        vec![vec![], vec![t("M8")], vec![t("M8"), t("M9")]],
        "M9 alone cannot be laid; nothing else exists"
    );
}

#[test]
fn generator_places_okey_everywhere() {
    let mut g = game(["K5 M5 OK Si9 S2", "", "", ""]);
    g.players[0].opened = Opened::Melds;
    put(&mut g, 1, "M6 M7 M8");
    let positions = generate(&g, 0, None, 1000).positions;
    let has = |want: &dyn Fn(&Act) -> bool| positions.iter().any(|acts| acts.iter().any(want));
    assert!(
        has(&|a| matches!(a, Act::Meld(Shape::Group { num: 5, jokers: 1, .. }))),
        "okey in a new group of fives"
    );
    assert!(
        has(&|a| matches!(
            a,
            Act::Layoff {
                tile: JOKER,
                side: Side::High,
                ..
            }
        )),
        "okey on the run's high end"
    );
    assert!(
        has(&|a| matches!(
            a,
            Act::Layoff {
                tile: JOKER,
                side: Side::Low,
                ..
            }
        )),
        "okey on the run's low end"
    );
    assert!(positions.iter().any(|a| a.is_empty()), "doing nothing is always an option");
    assert!(
        has(&|a| matches!(a, Act::Layoff { tile, side: Side::Low, .. } if *tile == t("M5"))),
        "M5 itself on the low end"
    );
}

#[test]
fn generator_optional_extra_meld() {
    // 101 exactly from two melds, plus a spare S2-S3-S4 that may be laid or kept.
    let g = game([&format!("{OPEN_101} S2 S3 S4 Si9"), "", "", ""]);
    let positions = generate(&g, 0, None, 5000).positions;
    let melds = |acts: &Vec<Act>| acts.iter().filter(|a| matches!(a, Act::Meld(_))).count();
    assert!(positions.iter().any(|a| melds(a) == 2), "open with just the two big runs");
    assert!(positions.iter().any(|a| melds(a) == 3), "open and lay the small run too");
    assert!(positions.iter().any(|a| a.is_empty()), "or do not open at all");
    assert!(!positions.iter().any(|a| melds(a) == 1), "one meld alone is under 101");
}

#[test]
fn analyzer_reports_moves() {
    let g = game([&format!("{OPEN_101} S2 S3 S4 Si9"), "", "", ""]);
    let mut view = public_view(&g, Phase::AfterDraw, None);
    view.deck_count = 10;
    for p in 1..4 {
        view.players[p].hand_len = Some(21);
    }
    let mut a = Analyzer::new(view.clone(), 3).unwrap();
    for _ in 0..12 {
        a.step(10);
    }
    let r = a.report();
    assert!(r.positions >= 3 && !r.truncated);
    let top = &r.candidates[0];
    let mv = top.mv.clone().expect("the best candidate carries its steps");
    assert!(matches!(top.decision, Decision::Play(_)));
    let steps = describe_move(&view, &mv);
    assert!(matches!(steps.last(), Some(Step::Discard { .. })));
}

#[test]
fn generator_runs_12_13_1() {
    let rules = Rules {
        one_after_thirteen: true,
        ..Rules::default()
    };
    let high = |jokers: u16| Shape::Run {
        color: 1,
        lo: 12,
        hi: 14,
        jokers,
    };
    let lays = |hand: &str, rules: &Rules, shape: Shape| {
        let mut g = game_with(rules.clone(), [hand, "", "", ""]);
        put(&mut g, 0, "K9 K10 K11");
        let positions = generate(&g, 0, None, 5000).positions;
        positions.iter().any(|acts| acts.contains(&Act::Meld(shape)))
    };
    assert!(lays("S12 S13 S1 K5", &rules, high(0)));
    assert!(lays("OK S13 S1 K5", &rules, high(1 << 12)));
    assert!(lays("S12 OK S1 K5", &rules, high(1 << 13)));
    assert!(lays("S12 S13 OK K5", &rules, high(1 << 14)), "the okey may stand for the 1");
    assert!(lays("OK OK S1 K5", &rules, high(1 << 12 | 1 << 13)));
    assert!(
        !lays("S12 S13 S1 K5", &Rules::default(), high(0)),
        "not at a table where 1 is only low"
    );
}

#[test]
fn public_view_keeps_thresholds() {
    let rules = Rules {
        raising: true,
        ..Rules::default()
    };
    let mut g = game_with(
        rules,
        [
            &format!("{OPEN_101} M5 M6 M7 Si8 Si2"),
            "K1 K1 M2 M2 S3 S3 Si4 Si4 K6 K6 M9 M9 Si1",
            "",
            "",
        ],
    );
    assert!(g.try_open_melds(&mut sv(&g), 0, None));
    assert!(g.try_open_pairs(1, None));
    assert!(g.meld_req > 101 && g.pair_req > 5);
    for seat in 0..4 {
        let view = public_view(&g.rotated(seat), Phase::BeforeDraw, None);
        assert_eq!(view.requirements(), (g.meld_req, g.pair_req), "seat {seat}");
    }
}

#[test]
fn finish_chance_excludes_partner() {
    let rules = Rules {
        partners: true,
        ..Rules::default()
    };
    let mut g = game_with(rules, ["K1 M4 S9 Si12 K6 M11", "", "", ""]);
    put(&mut g, 2, "K3 K4 K5");
    let mut view = public_view(&g, Phase::AfterDraw, None);
    view.deck_count = 30;
    view.players[1].hand_len = Some(21);
    view.players[3].hand_len = Some(21);
    view.players[2].hand_len = Some(1);
    view.players[2].known = vec![t("K2")];
    let mut a = Analyzer::new(view, 5).unwrap();
    a.step(60);
    let report = a.report();
    let top = &report.candidates[0];
    assert!(top.whole >= 30);
    assert!(top.p_finish < 0.2, "my partner goes out next turn, not me: {}", top.p_finish);
}

// ---------- playing for the match, not the hand ----------

use okey_engine::stakes::Stakes;

#[test]
fn last_hand_decides_match() {
    let close = Stakes {
        hands_left: 0,
        behind: [100, 100, 100],
    };
    assert_eq!(
        close.loss(&[-101, 101, 101, 60], false),
        0.0,
        "a 100-point deficit is overturned against all three"
    );
    assert_eq!(close.loss(&[-101, 101, 101, -50], false), 1.0, "one rival stays ahead: not first");
    let far = Stakes {
        hands_left: 0,
        behind: [300, 300, 300],
    };
    assert_eq!(far.loss(&[-101, 101, 101, 60], false), 1.0, "a plain finish cannot make up 300");
    assert_eq!(far.loss(&[-404, 404, 404, 240], false), 0.0, "elden with okey can");
}

#[test]
fn earlier_hands_leave_room() {
    let level = Stakes {
        hands_left: 5,
        behind: [0, 0, 0],
    };
    assert!(
        (level.loss(&[50, 50, 50, 50], false) - 0.875).abs() < 1e-6,
        "level with three rivals: one chance in eight"
    );
    let team = Stakes {
        hands_left: 3,
        behind: [0, 0, 0],
    };
    assert!(
        (team.loss(&[10, 20, 30, 20], true) - 0.5).abs() < 1e-6,
        "teams level after this hand"
    );
    let behind = Stakes {
        hands_left: 1,
        behind: [200, 0, 0],
    };
    let ahead = Stakes {
        hands_left: 1,
        behind: [-200, 0, 0],
    };
    assert!(behind.loss(&[0; 4], true) > 0.75 && ahead.loss(&[0; 4], true) < 0.25);
}

/// 18 tiles are my own melds; S9-S10 and M5 fit the table, and I hold an okey.
/// I can finish right now (in-one with the okey inside a run, or open and lay off), or keep waiting.
fn finish_or_wait(stakes: Option<Stakes>) -> View {
    let rules = Rules::default();
    let meld = |owner: u8, s: &str| TableMeld::new(owner, classify(&s.split_whitespace().map(t).collect::<Vec<_>>(), &rules).unwrap());
    let mut players = vec![PlayerView::default(); 4];
    players[3].opened = Opened::Melds;
    players[3].open_value = 120;
    players[3].hand_len = Some(9);
    players[1].hand_len = Some(21);
    players[2].hand_len = Some(21);
    players[1].discards = "K1 Si9 M7".split_whitespace().map(t).collect();
    players[2].discards = "Si1 K6 S13".split_whitespace().map(t).collect();
    players[3].discards = "M1 Si12".split_whitespace().map(t).collect();
    View {
        table: vec![
            meld(3, "S6 S7 S8"),
            meld(3, "K5 S5 Si5"),
            meld(3, "Si9 Si10 Si11 Si12"),
            meld(3, "K1 K2 K3"),
        ],
        indicator: t("M8"),
        hand: "K9 K10 K11 K12 K13 M10 M11 M12 M13 S1 S2 S3 Si5 Si6 Si7 K4 M4 Si4 S9 S10 M5 OK"
            .split_whitespace()
            .map(t)
            .collect(),
        players,
        pairs_added: [0; 4],
        deck_count: 12,
        phase: Phase::AfterDraw,
        taken: None,
        stakes,
        rules,
    }
}

fn ends_the_hand(view: &View, c: &CandReport) -> bool {
    let steps = match &c.mv {
        Some(mv) => describe_move(view, mv),
        None => describe(view, c.decision),
    };
    matches!(steps.last(), Some(Step::Discard { finish: true, .. }))
}

#[test]
fn sure_finish_without_stakes() {
    let view = finish_or_wait(None);
    let mut a = Analyzer::new(view.clone(), 3).unwrap();
    a.step(300);
    let r = a.report();
    assert!(ends_the_hand(&view, &r.candidates[0]), "best: {}", r.candidates[0].label);
    assert!(r.candidates[0].p_win.is_none());
}

#[test]
fn sure_finish_when_far_ahead() {
    let view = finish_or_wait(Some(Stakes {
        hands_left: 0,
        behind: [-400, -400, -400],
    }));
    let mut a = Analyzer::new(view.clone(), 3).unwrap();
    a.step(300);
    let r = a.report();
    assert!(ends_the_hand(&view, &r.candidates[0]), "best: {}", r.candidates[0].label);
    assert!(r.candidates[0].p_win.unwrap() > 0.99);
}

#[test]
fn hopeless_finish_refused() {
    // 500 behind everyone: finishing now (at best in-one, doubled) still loses the match.
    // Only an in-one finish that also throws the okey, four-fold, can turn it around, so the hand must go on.
    let view = finish_or_wait(Some(Stakes {
        hands_left: 0,
        behind: [500, 500, 500],
    }));
    let mut a = Analyzer::new(view.clone(), 3).unwrap();
    a.step(400);
    let r = a.report();
    let best = &r.candidates[0];
    assert!(!ends_the_hand(&view, best), "it must keep playing, but chose: {}", best.label);
    assert!(best.p_win.unwrap() > 0.0, "waiting keeps a real chance alive");
    assert!(
        describe(&view, best.decision).iter().all(|s| matches!(s, Step::Discard { .. })),
        "nothing goes on the table yet: {}",
        best.label
    );
    for c in r.candidates.iter().filter(|c| ends_the_hand(&view, c)) {
        assert!(c.p_win.unwrap() < 0.01, "finishing now cannot win: {} ({:?})", c.label, c.p_win);
    }
}

#[test]
fn who_draws_last_tile() {
    let rules = Rules::default();
    let mut g = Game::deal(&rules, &mut Rng::new(5), 0);
    g.deck.truncate(2);
    assert_eq!(g.draw(1).map(|_| g.last_draw), Some(None));
    assert!(g.draw(2).is_some());
    assert_eq!(g.last_draw, Some(2));
    assert!(g.draw(3).is_none());

    let view = View {
        rules: rules.clone(),
        indicator: t("S7"),
        hand: "K1 K2 K3 K4 K5 M9 M10 M11 S1 S1 Si4 Si5 Si6 K12 M12 S12 Si12 M3 M4 S9 Si13"
            .split_whitespace()
            .map(t)
            .collect(),
        players: (0..4)
            .map(|p| PlayerView {
                hand_len: (p > 0).then_some(21),
                discards: vec![[t("M1"), t("M2"), t("Si9"), t("K6")][p]],
                ..Default::default()
            })
            .collect(),
        table: vec![],
        pairs_added: [0; 4],
        deck_count: 5,
        phase: Phase::BeforeDraw,
        taken: None,
        stakes: None,
    };
    let opts = Options {
        policy: Policy::FINISHER,
        ..Options::default()
    };
    let mut a = Analyzer::with_options(view, 1, opts).unwrap();
    a.step(300);
    let r = a.report();
    let draw = r.candidates.iter().find(|c| c.decision == Decision::Draw).unwrap();
    let total: f64 = draw.p_last_draw.iter().sum();
    assert!(total > 0.0 && total <= 1.0 + 1e-9, "{:?}", draw.p_last_draw);
    assert!(
        draw.p_last_draw[0] > draw.p_last_draw[1],
        "with 5 tiles left and me drawing first, the last draw is mostly mine: {:?}",
        draw.p_last_draw
    );
}

#[test]
fn seven_pairs_minus_101() {
    use okey_engine::moves::Act;
    let pairs = |n: usize| -> Vec<Act> {
        ["K1", "K3", "K5", "S2", "S4", "M6", "M8"][..n]
            .iter()
            .map(|x| Act::Meld(Shape::Pair { tile: t(x), joker: false }))
            .collect()
    };
    let hand0 = "K1 K1 K3 K3 K5 K5 S2 S2 S4 S4 M6 M6 M8 M8 Si11 Si12 Si13 K9 K10 S11 S12 M13";
    let others = "K2 K4 K6 K7 K8 K9 K11 K12 K13 S1 S3 S5 S6 S8 S9 S10 S13 M1 M2 M3 M4";
    for (partners, n, want) in [(true, 7, [-101, 0, 0, 0]), (true, 6, [0, 0, 0, 0]), (false, 7, [-101, 0, 0, 0])] {
        let rules = Rules {
            partners,
            ..Rules::default()
        };
        let mut g = game_with(rules, [hand0, others, others, others]);
        g.play_acts(0, &pairs(n), None).unwrap();
        assert_eq!(g.players[0].opened, Opened::Pairs);
        assert_eq!(g.players.map(|p| p.penalty), want, "partners {partners}, {n} pairs");
    }
}

#[test]
fn fresh_run_stops_at_five() {
    use okey_engine::moves::Act;
    let run = |color, lo, hi| Act::Meld(Shape::Run { color, lo, hi, jokers: 0 });
    let lay = |meld, tile: &str, side| Act::Layoff { meld, tile: t(tile), side };
    let hand0 = "K8 K9 K10 K11 K12 K13 S9 S10 S11 S12 S13 M1 M2 M4 M5 M7 M8 Si1 Si2 Si4 Si5 Si7";
    let others = "K1 K2 K3 K4 K5 K6 K7 S1 S2 S3 S4 S5 S6 S7 S8 M3 M6 M9 M10 M11 M12";
    let fresh = || game([hand0, others, others, others]);

    let mut g = fresh();
    let five = [run(0, 9, 13), run(1, 9, 13), lay(0, "K8", Side::Low)];
    assert!(
        g.play_acts(0, &five, None).is_err(),
        "9-13 plus the 8 would be a run of six laid at once"
    );

    let mut g = fresh();
    let four = [run(0, 10, 13), run(1, 9, 13), lay(0, "K9", Side::Low)];
    assert!(g.play_acts(0, &four, None).is_ok(), "10-13 plus the 9 is a run of five");
    assert!(g.clone().play_acts(0, &[lay(0, "K8", Side::Low)], None).is_err());
    g.table.new_turn();
    assert!(
        g.play_acts(0, &[lay(0, "K8", Side::Low)], None).is_ok(),
        "next turn the run takes two per end again"
    );
}

#[test]
fn generator_run_after_group() {
    let g = game(["K1 S1 M1 K2 K3 K4 S7 S8 S9 M9 M10 M11 K12 M12 Si12 Si3 Si3", "", "", ""]);
    let positions = generate(&g, 0, Some(t("S1")), 5000).positions;
    let has = |acts: &Vec<Act>, want: &dyn Fn(&Shape) -> bool| acts.iter().any(|a| matches!(a, Act::Meld(s) if want(s)));
    assert!(
        positions.iter().any(|acts| {
            has(acts, &|s| matches!(s, Shape::Group { num: 1, .. }))
                && has(acts, &|s| {
                    matches!(
                        s,
                        Shape::Run {
                            color: 0,
                            lo: 2,
                            hi: 4,
                            ..
                        }
                    )
                })
        }),
        "the ones as a group, then K2-K3-K4: 102 with the taken S1"
    );
}

#[test]
fn generator_prefers_direct_layoff() {
    let g = game(["K13 S13 Si13 S12 M12 Si12 Si8 Si9 Si10 OK OK K2 K5", "", "", ""]);
    let positions = generate(&g, 0, None, 5000).positions;
    let plain = |acts: &&Vec<Act>| acts.len() == 3 && acts.iter().all(|a| matches!(a, Act::Meld(s) if s.jokers() == 0));
    assert_eq!(
        positions.iter().filter(plain).count(),
        1,
        "13s, 12s and Si8-Si9-Si10 straight from the hand"
    );
    assert!(
        !positions.iter().any(|acts| acts.iter().any(|a| matches!(a, Act::Swap(_)))),
        "taking back an okey laid this turn is the same position as laying the tiles themselves"
    );
}

#[test]
fn okey_layer_pays() {
    let lay = |p: u8| {
        let mut g = game(["K13 OK Si3 Si5", "K13 Si3", "Si3 Si4", "Si3 Si4"]);
        for q in 0..4 {
            g.players[q].opened = Opened::Melds;
        }
        let run = put(&mut g, 1, "K10 K11 K12");
        g.table.new_turn();
        let acts = [Act::Layoff {
            meld: run,
            tile: JOKER,
            side: Side::High,
        }];
        g.play_acts(0, &acts, None).expect("an okey on the end of another player's run");
        g.table.new_turn();
        let slot = g.table.joker_slots(&g.rules).remove(0);
        g.do_swap(p, &slot);
        g.players.map(|pl| pl.penalty)
    };
    assert_eq!(lay(0), [0, 0, 0, 0], "taking back the okey I laid costs nobody anything");
    assert_eq!(lay(1), [101, 0, 0, 0], "the owner of the run takes my okey: I laid it, I pay");
}

#[test]
fn generator_okey_beside_dead_twin() {
    let mut g = game(["K3 K3 K13 K13 S4 S4 Si1 Si1 M12 K9 M2 OK", "", "", ""]);
    let with = |g: &Game, tile: &str| {
        let want = Shape::Pair {
            tile: t(tile),
            joker: true,
        };
        generate(g, 0, None, 5000)
            .positions
            .iter()
            .any(|acts| acts.contains(&Act::Meld(want)))
    };
    assert!(
        with(&g, "M12") && !with(&g, "K9"),
        "nothing is known to be out: the okey goes beside the highest single"
    );
    g.seen[t("K9") as usize] += 1;
    assert!(with(&g, "K9"), "the other K9 is out, so an okey beside mine cannot be taken");
    assert!(with(&g, "M12"), "shedding the higher tile stays on offer");
}

#[test]
fn bot_uses_taken_tile() {
    let rules = Rules::default();
    let mut sv = Solver::new(&rules);
    let (mut takes, mut unused) = (0, Vec::new());
    for policy in [Policy::BASIC, Policy::TUNED, Policy::FINISHER, Policy::HOLDER, Policy::PATIENT] {
        for seed in 0..300u64 {
            let mut g = Game::deal(&rules, &mut Rng::new(seed), (seed % 4) as u8);
            g.policy = [policy; 4];
            let mut over = g.starter_turn(&mut sv).is_some();
            while !over && !g.deck.is_empty() {
                let p = g.current;
                g.table.new_turn();
                let taken = g.bot_wants_take(&mut sv, p).then(|| {
                    let before = g.players[p as usize].hand;
                    (g.take_top(p), before)
                });
                if taken.is_none() && g.draw(p).is_none() {
                    break;
                }
                over = g.bot_act(&mut sv, p, taken.map(|x| x.0)).is_some();
                if let Some((tile, before)) = taken {
                    takes += 1;
                    let thrown = u8::from(g.last[p as usize] == Some(tile));
                    if !over && g.players[p as usize].hand[tile as usize] + thrown > before[tile as usize] {
                        unused.push((seed, p, short(tile)));
                    }
                }
                g.current = (p + 1) % 4;
            }
        }
    }
    assert!(takes > 1000, "{takes}");
    assert!(
        unused.is_empty(),
        "{} of {takes} takes left the tile unused: {:?}",
        unused.len(),
        &unused[..unused.len().min(8)]
    );
}
