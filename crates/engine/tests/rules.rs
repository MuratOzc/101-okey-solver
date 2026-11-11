//! One test per table rule the user specified. A failing test here means the engine
//! is recommending or simulating something that is illegal at the user's table.

use okey_engine::analysis::*;
use okey_engine::game::*;
use okey_engine::meld::*;
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
        gosterge: t("S7"),
        players: hands.map(|h| Player {
            hand: hand(h),
            opened: Opened::No,
            gosterge_bonus: false,
            penalty: 0,
        }),
        deck: vec![],
        table: Table::default(),
        top: None,
        current: 0,
        meld_req: rules.open_min,
        pair_req: rules.pair_min,
        seen,
        log: None,
        policy: [Default::default(); 4],
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
fn opening_with_melds_needs_at_least_101() {
    let mut g = game([&format!("{OPEN_100} Si8"), "", "", ""]);
    assert!(!g.try_open_melds(&mut sv(&g), 0, None), "100 points must not open");
    assert_eq!(g.players[0].opened, Opened::No);

    let mut g = game([&format!("{OPEN_101} Si8 Si2"), "", "", ""]);
    assert!(g.try_open_melds(&mut sv(&g), 0, None), "exactly 101 opens");
    assert_eq!(g.players[0].opened, Opened::Melds);
    assert_eq!(g.table.melds.len(), 2);
}

#[test]
fn opening_with_pairs_needs_five_pairs() {
    let mut g = game(["K1 K1 M2 M2 S3 S3 Si4 Si4 K6 M9", "", "", ""]);
    assert!(!g.try_open_pairs(0, None), "four pairs must not open");
    let mut g = game(["K1 K1 M2 M2 S3 S3 Si4 Si4 K6 K6 M9", "", "", ""]);
    assert!(g.try_open_pairs(0, None));
    assert_eq!(g.players[0].opened, Opened::Pairs);
}

#[test]
fn okey_with_a_single_is_a_pair_but_two_okeys_are_not() {
    let mut g = game(["K1 K1 M2 M2 S3 S3 Si4 Si4 K6 OK M9", "", "", ""]);
    assert!(g.try_open_pairs(0, None), "okey + K6 is the fifth pair");
    let mut g = game(["K1 K1 M2 M2 S3 S3 Si4 Si4 OK OK", "", "", ""]);
    assert!(!g.try_open_pairs(0, None), "two okeys do not make the fifth pair");
}

#[test]
fn gosterge_dealt_to_you_pairs_with_any_tile() {
    let h = "K1 K1 M2 M2 S3 S3 Si4 Si4 S7 K11 M9";
    let mut g = game([h, "", "", ""]);
    assert!(!g.try_open_pairs(0, None), "without the gösterge right it is only four pairs");
    let mut g = game([h, "", "", ""]);
    g.players[0].gosterge_bonus = true;
    assert!(g.try_open_pairs(0, None), "gösterge (S7) + K11 counts as the fifth pair");
}

#[test]
fn katlamali_raises_each_threshold_separately() {
    let rules = Rules {
        katlamali: true,
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
fn katlamasiz_keeps_thresholds_fixed() {
    let mut g = game([&format!("{OPEN_101} Si8"), "S9 S10 S11 S12 S13 Si10 Si11 Si12 Si13 K1", "", ""]);
    assert!(g.try_open_melds(&mut sv(&g), 0, None));
    assert_eq!(g.meld_req, 101);
    assert!(g.try_open_melds(&mut sv(&g), 1, None));
}

// ---------- nothing on the table before opening ----------

#[test]
fn unopened_player_cannot_lay_off_or_take_okey() {
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
    assert!(!g.elden_possible(&mut sv(&g), 0, None));
}

#[test]
fn laying_off_the_whole_hand_is_not_an_opening() {
    // K1-K2-K3 is only 6 points: M8 may not go onto the table run just because the hand would empty.
    let mut g = game(["M8 K1 K2 K3 Si9", "", "", ""]);
    put(&mut g, 1, "M5 M6 M7");
    assert!(!g.elden_possible(&mut sv(&g), 0, None));
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
fn no_recommendation_touches_the_table_before_opening() {
    for katlamali in [false, true] {
        let rules = Rules {
            katlamali,
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

// ---------- elden bitme ----------

const ALL_MELDS: &str = "K1 K2 K3 M1 M2 M3 S1 S2 S3 Si1 Si2 Si3 K5 M5 S5 K7 M7 Si7 K9 M9 S9";

#[test]
fn elden_needs_no_101_but_only_own_melds() {
    let mut g = game([&format!("{ALL_MELDS} Si13"), "K5", "K5", "K5"]);
    let out = g.apply_choice(&mut sv(&g), 0, OpenChoice::Finish, None).unwrap().unwrap();
    let Outcome::Finished(f) = out else { panic!("should finish") };
    assert!(f.elden && !f.okey && !f.pairs);
    assert_eq!(f.last, t("Si13"));
    assert_eq!(g.scores(&out), [-202, 202, 202, 202], "elden doubles everything");
}

#[test]
fn opening_then_laying_off_to_finish_is_a_normal_finish() {
    let mut g = game([&format!("{OPEN_101} M8 Si2"), "K5", "", ""]);
    put(&mut g, 1, "M5 M6 M7");
    assert!(!g.elden_possible(&mut sv(&g), 0, None));
    let out = g.apply_choice(&mut sv(&g), 0, OpenChoice::OPEN, None).unwrap().unwrap();
    let Outcome::Finished(f) = out else { panic!("should finish") };
    assert!(!f.elden, "opening and finishing by laying off is not elden");
    assert_eq!(g.scores(&out)[0], -101);
}

#[test]
fn finishing_by_discarding_okey_doubles() {
    let mut g = game([&format!("{ALL_MELDS} OK"), "K5", "", ""]);
    let out = g.apply_choice(&mut sv(&g), 0, OpenChoice::Finish, None).unwrap().unwrap();
    let Outcome::Finished(f) = out else { panic!("should finish") };
    assert!(f.okey && f.elden);
    assert_eq!(g.scores(&out)[..2], [-404, 404], "okey × elden multiply to four");
}

// ---------- scoring ----------

fn finished(player: u8, okey: bool, elden: bool, pairs: bool) -> Outcome {
    Outcome::Finished(FinishInfo {
        player,
        okey,
        elden,
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
fn okey_left_in_hand_costs_101_each() {
    let mut g = game(["", "K5 OK", "K5 OK", ""]);
    g.players[2].opened = Opened::Melds;
    let s = g.scores(&finished(0, false, false, false));
    assert_eq!(s[1], 202, "unopened 101 + okey 101");
    assert_eq!(s[2], 106, "5 + okey 101");
}

#[test]
fn in_hand_penalties_are_not_multiplied() {
    let mut g = game(["", "K5", "", ""]);
    g.players[1].penalty = 101;
    assert_eq!(g.scores(&finished(0, true, false, false))[1], 202 + 101);
}

#[test]
fn deck_out_counts_normally_with_no_finisher() {
    let mut g = game(["K5 M9", "K5", "", ""]);
    g.players[0].opened = Opened::Melds;
    assert_eq!(g.scores(&Outcome::DeckOut)[..2], [14, 101]);
}

#[test]
fn partner_finishing_gives_the_team_minus_202() {
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
fn islek_stays_islek_after_this_turns_two_layoffs() {
    let mut g = game(["M11 K1", "", "", ""]);
    let i = put(&mut g, 1, "M6 M7 M8");
    assert!(g.table.lay_off_on(i, t("M9"), None, &g.rules.clone()).is_some());
    assert!(g.table.lay_off_on(i, t("M10"), None, &g.rules.clone()).is_some());
    assert_eq!(g.discard_penalty(0, t("M11")), 101);
}

// ---------- taking the side tile ----------

#[test]
fn side_tile_only_if_it_is_used_this_turn() {
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
fn taking_must_open_with_that_tile_not_just_open() {
    // The hand already opens without S2; S2 itself fits nothing, so it may not be taken.
    let g = game([&format!("{OPEN_101} Si8"), "", "", ""]);
    assert!(!g.can_use_taken(&mut sv(&g), 0, t("S2")));
}

#[test]
fn optional_side_take_penalty_is_ten_times_the_tile() {
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

// ---------- after opening ----------

#[test]
fn pair_opener_adds_pairs_and_lays_off_but_never_lays_runs() {
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
fn at_most_three_pairs_per_turn_into_a_pair_area() {
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
fn meld_opener_gives_pairs_to_pair_openers_three_each() {
    let mut g = game(["K1 K1 M2 M2 S3 S3 Si4 Si4 K6 K6 M9 M9 S11 S11 K9", "", "", ""]);
    g.players[0].opened = Opened::Melds;
    put(&mut g, 1, "K13 K13");
    put(&mut g, 2, "M13 M13");
    assert!(g.lay(&mut sv(&g), 0, None, false).unwrap().is_none());
    assert_eq!(g.table.pairs_added, [0, 3, 3, 0], "three pairs to each pair opener");
    assert_eq!(total(&g.players[0].hand), 3);
}

#[test]
fn meld_opener_cannot_open_pairs_and_no_pair_area_means_no_pairs() {
    let mut g = game(["K1 K1 M2 M2 S3 S3 Si4 Si4 K6 K6 K9", "", "", ""]);
    g.players[0].opened = Opened::Melds;
    assert!(g.clone().apply_choice(&mut sv(&g), 0, OpenChoice::PAIRS, None).is_err());
    assert!(g.lay(&mut sv(&g), 0, None, false).unwrap().is_none());
    assert!(g.table.melds.is_empty(), "nobody opened with pairs, so pairs stay in hand");
}

#[test]
fn new_runs_are_three_to_five_long() {
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
fn a_tile_is_kept_back_to_discard() {
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
fn losing_okey_to_an_opponent_costs_the_owner_101() {
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
fn opened_players_take_an_exposed_okey_when_they_hold_the_tile() {
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
fn partner_okey_is_free_in_partner_mode() {
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
fn group_okey_needs_the_group_completed() {
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
fn one_is_only_low_and_groups_need_distinct_colors() {
    let r = Rules::default();
    assert!(classify(&[t("K12"), t("K13"), t("K1")], &r).is_none(), "12-13-1 is not a run");
    assert!(classify(&[t("K1"), t("K2"), t("K3")], &r).is_some());
    assert!(
        classify(&[t("K5"), t("K5"), t("M5")], &r).is_none(),
        "the same color twice is not a group"
    );
    assert!(classify(&[t("K5"), t("S5"), t("M5"), t("Si5")], &r).is_some());
}

#[test]
fn okey_is_the_tile_after_the_gosterge_and_scores_as_what_it_replaces() {
    assert_eq!(okey_of(t("S7")), t("S8"));
    assert_eq!(okey_of(t("K13")), t("K1"));
    let r = Rules::default();
    assert_eq!(classify(&[t("K10"), JOKER, t("K12")], &r).unwrap().open_value(&r), 33);
    assert_eq!(classify(&[t("K10"), t("M10"), JOKER], &r).unwrap().open_value(&r), 30);
}

#[test]
fn starter_gets_22_and_twenty_tiles_stay_in_the_deck() {
    let rules = Rules::default();
    let g = Game::deal(&rules, &mut Rng::new(5), 2);
    assert_eq!(total(&g.players[2].hand), 22);
    assert!([0, 1, 3].iter().all(|&p| total(&g.players[p].hand) == 21));
    assert_eq!(g.deck.len(), 20);
}

#[test]
fn okey_can_be_kept_out_of_runs() {
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
fn safe_opening_never_puts_okey_in_a_run() {
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
fn every_generated_move_is_legal_and_opens_before_touching_the_table() {
    for katlamali in [false, true] {
        let rules = Rules {
            katlamali,
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
fn generator_finds_exactly_the_layoff_options() {
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
fn generator_offers_every_place_for_the_okey() {
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
fn generator_lists_openings_with_and_without_the_extra_meld() {
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
fn exhaustive_analyzer_reports_explicit_moves() {
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
