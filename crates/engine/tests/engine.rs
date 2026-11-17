use okey_engine::analysis::*;
use okey_engine::game::*;
use okey_engine::meld::*;
use okey_engine::pairs::pair_plan;
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

fn open_value(h: &str) -> i32 {
    let r = Rules::default();
    let mut sv = Solver::new(&r);
    let p = Params {
        mode: Mode::Open,
        cover_w: 0,
        must: None,
        allow_melds: true,
        pair_dump: false,
        pair_cap: 0,
        joker_runs: true,
    };
    sv.run(&hand(h), p, None).map_or(0, |p| p.open_value(&r))
}

#[test]
fn tiles_and_okey() {
    assert_eq!(okey_of(t("S7")), t("S8"));
    assert_eq!(okey_of(t("M13")), t("M1"));
    assert_eq!(t("Si4"), id(3, 4));
    assert_eq!(parse("ok"), Some(JOKER));
}

#[test]
fn open_values() {
    assert_eq!(open_value("K10 K11 K12 K13 M13 S13 Si13 M7 M8 M9"), 46 + 39 + 24);
    assert_eq!(open_value("K10 K11 OK"), 33);
    assert_eq!(open_value("K5 M5 OK"), 15);
    assert_eq!(open_value("K12 K13 K1"), 0, "12-13-1 is not a run by default");
    assert_eq!(open_value("K1 K2 K3"), 6);
    assert_eq!(open_value("K5 K5 M5 S5"), 15);
    assert_eq!(open_value("K5 K5 M5 M5 S5 S5"), 30, "two groups from duplicates");
}

#[test]
fn run_length_capped_at_five() {
    assert_eq!(open_value("M1 M2 M3 M4 M5 M6"), 21, "a six-run must be two threes");
    assert_eq!(open_value("M1 M2 M3 M4 M5 M6 M7"), 28, "3 + 4 split uses every tile");
    assert_eq!(open_value("M1 M2 M3 M4 M5 M6 M7 M8 M9 M10 M11"), 66, "five + three + three");
}

#[test]
fn ace_after_king_option() {
    let r = Rules {
        ace_after_king: true,
        ace_high_value: 1,
        ..Rules::default()
    };
    let mut sv = Solver::new(&r);
    let p = Params {
        mode: Mode::Open,
        cover_w: 0,
        must: None,
        allow_melds: true,
        pair_dump: false,
        pair_cap: 0,
        joker_runs: true,
    };
    let plan = sv.run(&hand("K12 K13 K1"), p, None).unwrap();
    assert_eq!(plan.open_value(&r), 26);
}

#[test]
fn must_use_constraint() {
    let r = Rules::default();
    let mut sv = Solver::new(&r);
    let p = Params {
        mode: Mode::Open,
        cover_w: 0,
        must: Some(t("K9")),
        allow_melds: true,
        pair_dump: false,
        pair_cap: 0,
        joker_runs: true,
    };
    assert!(sv.run(&hand("K9 M1 M2 M3"), p, None).is_none());
    let plan = sv.run(&hand("K9 M9 S9 M1 M2 M3"), p, None).unwrap();
    assert!(plan.melds.iter().any(|m| m.uses(t("K9"))));
}

#[test]
fn penalty_mode_minimizes_deadwood() {
    let r = Rules::default();
    let mut sv = Solver::new(&r);
    let p = Params {
        mode: Mode::Penalty,
        cover_w: 1000,
        must: None,
        allow_melds: true,
        pair_dump: false,
        pair_cap: 0,
        joker_runs: true,
    };
    let plan = sv.run(&hand("K3 K4 K5 M9 Si13"), p, None).unwrap();
    let mut rest = plan.rest.clone();
    rest.sort();
    assert_eq!(rest, vec![t("M9"), t("Si13")]);
}

#[test]
fn pairs_counting() {
    let r = Rules::default();
    assert_eq!(pair_plan(&hand("K1 K1 M2 M2 S3 OK"), &r, None).pairs.len(), 3);
    assert_eq!(pair_plan(&hand("OK OK"), &r, None).pairs.len(), 0, "two okeys are not a pair");
    let g = t("S7");
    assert_eq!(
        pair_plan(&hand("S7 K11"), &r, Some(g)).pairs.len(),
        1,
        "gösterge pairs with any tile"
    );
}

#[test]
fn classify_melds() {
    let r = Rules::default();
    assert_eq!(
        classify(&[t("K5"), JOKER, t("K7")], &r),
        Some(Shape::Run {
            color: 0,
            lo: 5,
            hi: 7,
            jokers: 1 << 6
        })
    );
    assert!(matches!(
        classify(&[t("K5"), t("M5"), JOKER], &r),
        Some(Shape::Group { num: 5, .. })
    ));
    assert!(classify(&[t("K5"), t("M6"), t("S7")], &r).is_none());
}

fn table_with(shape: Shape) -> Table {
    Table {
        melds: vec![TableMeld::new(1, shape)],
        pairs_added: [0; 4],
    }
}

#[test]
fn layoff_two_per_end_per_turn() {
    let r = Rules::default();
    let mut tb = table_with(classify(&[t("M6"), t("M7"), t("M8")], &r).unwrap());
    assert!(tb.lay_off(t("M9"), &r, true).is_some());
    assert!(tb.lay_off(t("M10"), &r, true).is_some());
    assert!(tb.lay_off(t("M11"), &r, true).is_none(), "third tile on one end is not allowed");
    assert!(tb.lay_off(t("M5"), &r, true).is_some());
    assert!(tb.lay_off(JOKER, &r, true).is_some(), "okey counts toward the low end");
    assert!(tb.lay_off(t("M3"), &r, true).is_none());
    tb.new_turn();
    assert!(tb.lay_off(t("M11"), &r, true).is_some(), "limit resets next turn");
}

#[test]
fn islek_ignores_turn_counters() {
    let r = Rules::default();
    let mut tb = table_with(classify(&[t("M6"), t("M7"), t("M8")], &r).unwrap());
    tb.lay_off(t("M9"), &r, true);
    tb.lay_off(t("M10"), &r, true);
    assert!(tb.is_islek(t("M11"), &r));
}

#[test]
fn group_okey_needs_all_missing_colors() {
    let r = Rules::default();
    let tb = table_with(classify(&[t("S8"), t("M8"), JOKER], &r).unwrap());
    let slots = tb.joker_slots(&r);
    assert_eq!(slots.len(), 1);
    let mut need = slots[0].tiles.clone();
    need.sort();
    assert_eq!(need, vec![t("K8"), t("Si8")]);

    let loose = Rules {
        group_joker_must_complete: false,
        ..Rules::default()
    };
    assert_eq!(tb.joker_slots(&loose).len(), 2);

    let tb4 = table_with(classify(&[t("S8"), t("M8"), t("K8"), JOKER], &r).unwrap());
    assert_eq!(tb4.joker_slots(&r)[0].tiles, vec![t("Si8")]);

    let two = table_with(classify(&[t("S8"), t("M8"), JOKER, JOKER], &r).unwrap());
    let slots = two.joker_slots(&r);
    assert_eq!(
        slots.iter().filter(|s| s.jokers == 1 && s.tiles.len() == 1).count(),
        2,
        "either missing color frees one okey"
    );
    assert!(slots.iter().any(|s| s.jokers == 2 && s.tiles.len() == 2));
}

#[test]
fn full_games_conserve_tiles() {
    for partners in [false, true] {
        let rules = Rules {
            partners,
            ..Rules::default()
        };
        let mut sv = Solver::new(&rules);
        let mut finished = 0;
        for seed in 0..300u64 {
            let mut rng = Rng::new(seed);
            let mut g = Game::deal(&rules, &mut rng, (seed % 4) as u8);
            assert_eq!(g.tile_count(), 106);
            let out = match g.starter_turn(&mut sv) {
                Some(o) => o,
                None => g.play_out(&mut sv),
            };
            assert_eq!(g.tile_count(), 106, "seed {seed}");
            let s = g.scores(&out);
            if let Outcome::Finished(f) = out {
                finished += 1;
                let penalties = g.players[f.player as usize].penalty;
                assert!(
                    s[f.player as usize] - penalties < 0,
                    "apart from penalties the finisher scores negative"
                );
            }
        }
        assert!(finished > 50, "bots should finish a fair share of hands, got {finished}");
    }
}

fn sample_view(phase: Phase) -> View {
    let rules = Rules::default();
    let mut rng = Rng::new(42);
    let g = Game::deal(&rules, &mut rng, 0);
    let mut hand = tiles_of(&g.players[0].hand);
    if phase == Phase::BeforeDraw {
        hand.pop();
    }
    let mut players = vec![PlayerView::default(); 4];
    let top = tiles_of(&g.players[3].hand)[0];
    players[3].discards.push(top);
    View {
        rules,
        gosterge: g.gosterge,
        hand,
        players,
        table: vec![],
        pairs_added: [0; 4],
        deck_count: 15,
        phase,
        taken: None,
        stakes: None,
    }
}

#[test]
fn analyzer_runs_both_phases() {
    for phase in [Phase::BeforeDraw, Phase::AfterDraw] {
        let mut a = Analyzer::new(sample_view(phase), 9).unwrap();
        a.step(60);
        let r = a.report();
        assert!(!r.candidates.is_empty());
        assert!(r.candidates[0].mean.is_finite());
    }
}

#[test]
fn hand_report_lists_outs() {
    let v = sample_view(Phase::AfterDraw);
    let rep = hand_report(&v);
    assert_eq!(rep.unseen_total, 106 - v.hand.len() as u32 - 1 - 1);
    assert!(rep.problems.is_empty());
}

#[test]
fn okey_side_can_be_chosen() {
    let r = Rules::default();
    let mut tb = table_with(classify(&[t("M6"), t("M7"), t("M8")], &r).unwrap());
    assert_eq!(tb.lay_off_on(0, JOKER, Some(Side::Low), &r), Some(Side::Low));
    assert_eq!(
        tb.melds[0].shape,
        Shape::Run {
            color: 2,
            lo: 5,
            hi: 8,
            jokers: 1 << 5
        }
    );
    let opts = tb.melds[0].options(&r);
    assert!(
        opts.iter()
            .any(|o| o.tile == JOKER && o.side == Side::High && o.stands_for == t("M9"))
    );
    assert!(opts.iter().any(|o| o.tile == t("M4") && o.side == Side::Low));
}

#[test]
fn describe_lists_physical_steps() {
    let rules = Rules::default();
    let hand: Vec<Tile> = "K1 K2 K3 K4 K5 M9 M10 M11 S1 S1 Si4 Si5 Si6 K12 M12 S12 Si12 M3 M4 S9 Si13 K13"
        .split_whitespace()
        .map(t)
        .collect();
    let view = View {
        rules,
        gosterge: t("S7"),
        hand,
        players: vec![PlayerView::default(); 4],
        table: vec![],
        pairs_added: [0; 4],
        deck_count: 20,
        phase: Phase::AfterDraw,
        taken: None,
        stakes: None,
    };
    let steps = describe(
        &view,
        Decision::Act {
            choice: OpenChoice::OPEN,
            discard: Some(t("S9")),
        },
    );
    let melds = steps.iter().filter(|s| matches!(s, Step::Meld { .. })).count();
    assert!(melds >= 3, "opening lays several melds, got {steps:?}");
    assert_eq!(
        steps.last(),
        Some(&Step::Discard {
            tile: t("S9"),
            finish: false
        })
    );
}

#[test]
fn deals_played_elsewhere_count_the_same_as_deals_played_here() {
    let view = sample_view(Phase::AfterDraw);
    let mut alone = Analyzer::new(view.clone(), 1).unwrap();
    alone.step(150);
    let mut boss = Analyzer::new(view.clone(), 1).unwrap();
    let mut hand = Analyzer::new(view, 99).unwrap();
    for k in 0..150u64 {
        let field = boss.field();
        hand.set_field(&field);
        let deal = hand.evaluate(1u64.wrapping_add(k.wrapping_mul(0x2545_F491_4F6C_DD1D)));
        boss.absorb(field.version, &[deal]);
    }
    let rows = |a: &Analyzer| {
        let r = a.report();
        (
            r.iterations,
            r.candidates
                .iter()
                .map(|c| (c.decision, c.n, c.mean.to_bits(), c.alive))
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(rows(&alone), rows(&boss));
}

#[test]
fn late_results_count_only_for_what_is_still_alive() {
    let view = sample_view(Phase::AfterDraw);
    let mut boss = Analyzer::new(view.clone(), 1).unwrap();
    let mut hand = Analyzer::new(view, 2).unwrap();
    let first = boss.field();
    hand.set_field(&first);
    let batch: Vec<_> = (0..200u64).map(|k| hand.evaluate(k)).collect();
    boss.absorb(first.version, &batch);
    let r = boss.report();
    assert_eq!(r.iterations, 200, "a field that only lost candidates keeps taking the old results");
    assert!(boss.version() > first.version, "200 deals prune something");
    assert!(boss.field().cands.len() < first.cands.len(), "the pruned ones left the field");
    assert!(r.candidates.iter().filter(|c| !c.alive).all(|c| c.n < 200));
    assert!(r.candidates.iter().filter(|c| c.alive).all(|c| c.n == 200));
    boss.absorb(first.version + 1000, &batch);
    assert_eq!(boss.report().iterations, 200, "results for a field never sent are ignored");
}

#[test]
fn inference_makes_ready_but_unopened_opponents_rarer() {
    let mut view = sample_view(Phase::AfterDraw);
    view.deck_count = 8;
    for p in 1..4 {
        view.players[p].discards = vec![t(["K1", "M2", "Si3"][p - 1])];
    }
    let mut sv = Solver::new(&view.rules);
    let ready = |infer: bool, sv: &mut Solver| {
        let opts = Options {
            infer,
            ..Options::default()
        };
        (0..400u64)
            .filter(|&seed| {
                let g = view.sample(&mut Rng::new(seed), sv, &opts);
                g.open_search(sv, &g.players[1].hand, None)
                    .is_some_and(|p| p.open_value(&g.rules) >= g.meld_req)
            })
            .count()
    };
    let (plain, inferred) = (ready(false, &mut sv), ready(true, &mut sv));
    assert!(
        plain > 20,
        "uniform deals often hand an unopened opponent a ready opening ({plain}/400)"
    );
    assert!(inferred * 2 < plain, "inference should make that much rarer: {inferred} vs {plain}");
}

#[test]
fn lookahead_playouts_keep_every_tile_and_end_the_hand() {
    let rules = Rules::default();
    let mut sv = Solver::new(&rules);
    for seed in 0..60u64 {
        let mut g = Game::deal(&rules, &mut Rng::new(seed), 1);
        if g.starter_turn(&mut sv).is_some() {
            continue;
        }
        let out = g.play_out_lookahead(&mut sv, 0, 2);
        assert_eq!(g.tile_count(), 106, "seed {seed}");
        let s = g.scores(&out);
        assert!(s.iter().all(|x| x.abs() < 2000));
    }
}

#[test]
fn analyzer_switches_to_lookahead_once_few_moves_remain() {
    let opts = Options {
        lookahead: true,
        ..Options::default()
    };
    let mut a = Analyzer::with_options(sample_view(Phase::BeforeDraw), 5, opts).unwrap();
    for _ in 0..8 {
        a.step(10);
    }
    let r = a.report();
    assert!(r.deep, "two candidates (draw / take) are few enough for look-ahead");
    assert!(r.candidates[0].n > 0 && r.candidates[0].mean.is_finite());
}

#[test]
fn rotating_seats_round_trips() {
    let rules = Rules::default();
    let mut sv = Solver::new(&rules);
    let mut g = Game::deal(&rules, &mut Rng::new(11), 2);
    g.starter_turn(&mut sv);
    for _ in 0..6 {
        if g.bot_turn(&mut sv).is_some() {
            break;
        }
    }
    let turned = g.rotated(3);
    assert_eq!(turned.players[0].hand, g.players[3].hand);
    assert_eq!(turned.current, (g.current + 1) % 4);
    let back = turned.rotated(1);
    assert_eq!(back.current, g.current);
    assert!((0..4).all(|p| back.players[p].hand == g.players[p].hand));
    assert!(
        back.table
            .melds
            .iter()
            .zip(&g.table.melds)
            .all(|(a, b)| a.owner == b.owner && a.shape == b.shape)
    );
}

#[test]
fn elo_follows_head_to_head_results() {
    use okey_engine::agent::elo_from_pairs;
    // A beats B ten times in eleven: that is what a 400-point lead means.
    let r = elo_from_pairs(&[vec![0.0, 100.0], vec![10.0, 0.0]], 1000.0);
    assert!((r[0] - 1000.0).abs() < 1e-6);
    assert!((r[1] - 600.0).abs() < 1.0, "got {}", r[1]);
    let even = elo_from_pairs(&[vec![0.0, 50.0], vec![50.0, 0.0]], 1000.0);
    assert!((even[1] - 1000.0).abs() < 1.0);
}

#[test]
fn agents_play_a_full_hand_from_any_seat() {
    use okey_engine::agent::*;
    use okey_engine::policy::Policy;
    let rules = Rules::default();
    let mut sv = Solver::new(&rules);
    let mut g = Game::deal(&rules, &mut Rng::new(4), 3);
    let mut bots: Vec<BotAgent> = (0..3)
        .map(|i| BotAgent {
            label: format!("bot{i}"),
            policy: Policy::BASIC,
        })
        .collect();
    let mut search = SearchAgent {
        label: "search".into(),
        opts: Options {
            lookahead: false,
            ..Options::default()
        },
        iterations: 25,
        seed: 7,
    };
    let [a, b, c] = &mut bots[..] else { unreachable!() };
    let mut seats: [&mut dyn Agent; 4] = [a, b, &mut search, c];
    let scores = play_hand(&mut g, &mut sv, &mut seats);
    assert_eq!(g.tile_count(), 106);
    assert!(scores.iter().all(|s| s.abs() < 2000));
}

/// Bot play must stay exactly the same when the engine is only being made faster.
/// If this fails after an intended change of behaviour, update the numbers.
#[test]
fn bot_play_is_unchanged() {
    use okey_engine::policy::Policy;
    let rules = Rules::default();
    let mut sv = Solver::new(&rules);
    let mut sums = Vec::new();
    for policy in [Policy::BASIC, Policy::TUNED] {
        let mut h = 0u64;
        for seed in 0..400u64 {
            let mut g = Game::deal(&rules, &mut Rng::new(seed), (seed % 4) as u8);
            g.policy = [policy; 4];
            let o = g.starter_turn(&mut sv).unwrap_or_else(|| g.play_out(&mut sv));
            for s in g.scores(&o) {
                h = h.wrapping_mul(1_000_003).wrapping_add(s as i64 as u64);
            }
        }
        sums.push(h);
    }
    assert_eq!(sums, GOLDEN, "bot behaviour changed");
}

const GOLDEN: [u64; 2] = [8850612239179389771, 9617653818221818233];

/// The cheap pre-checks may only ever say "impossible" when the full search agrees.
#[test]
fn quick_bounds_never_contradict_the_search() {
    for ace in [false, true] {
        let rules = Rules {
            ace_after_king: ace,
            ace_high_value: if ace { 11 } else { 1 },
            ..Rules::default()
        };
        let mut sv = Solver::new(&rules);
        let open = Params {
            mode: Mode::Open,
            cover_w: 0,
            must: None,
            allow_melds: true,
            pair_dump: false,
            pair_cap: 0,
            joker_runs: true,
        };
        let cover = Params {
            mode: Mode::Penalty,
            cover_w: 1000,
            ..open
        };
        for seed in 0..1500u64 {
            let g = Game::deal(&rules, &mut Rng::new(seed), 0);
            for p in 0..4 {
                let hand = g.players[p].hand;
                let value = sv.run(&hand, open, None).map_or(0, |pl| pl.open_value(&rules));
                assert!(
                    open_bound(&hand, &rules) >= value,
                    "seed {seed}: bound below the real opening value {value}"
                );
                let left = sv.run(&hand, cover, None).map_or(99, |pl| pl.rest.len() as u32);
                assert!(
                    stranded(&hand, &rules, 99) <= left,
                    "seed {seed}: more stranded tiles than the best arrangement leaves"
                );
            }
        }
    }
}

/// Splitting a hand into independent parts must find exactly as good an arrangement as one big search,
/// for every kind of question the engine asks.
#[test]
fn split_search_matches_the_whole_search() {
    for ace in [false, true] {
        let rules = Rules {
            ace_after_king: ace,
            ..Rules::default()
        };
        let mut split = Solver::new(&rules);
        let mut whole = Solver::new(&rules);
        let open = Params {
            mode: Mode::Open,
            cover_w: 0,
            must: None,
            allow_melds: true,
            pair_dump: false,
            pair_cap: 0,
            joker_runs: true,
        };
        let mut compared = 0;
        for seed in 0..700u64 {
            let g = Game::deal(&rules, &mut Rng::new(seed), 0);
            let mut attach = Attach::none();
            let mut rng = Rng::new(seed ^ 77);
            for _ in 0..6 {
                attach.tile[rng.below(NIDS)] = true;
            }
            for p in 0..4 {
                let hand = g.players[p].hand;
                let some_tile = tiles_of(&hand)[rng.below(20)];
                for params in [
                    open,
                    Params { joker_runs: false, ..open },
                    Params {
                        must: Some(some_tile),
                        ..open
                    },
                    Params {
                        mode: Mode::Penalty,
                        cover_w: 1000,
                        ..open
                    },
                    Params {
                        mode: Mode::Penalty,
                        cover_w: 1000,
                        must: Some(some_tile),
                        ..open
                    },
                ] {
                    for att in [None, Some(&attach)] {
                        let a = split.run(&hand, params, att);
                        let b = whole.run_whole(&hand, params, att);
                        assert_eq!(
                            a.as_ref().map(|x| x.score),
                            b.as_ref().map(|x| x.score),
                            "seed {seed} seat {p} {params:?}"
                        );
                        if let Some(plan) = a {
                            let mut used = counts_from(&plan.rest);
                            plan.layoffs.iter().for_each(|&t| used[t as usize] += 1);
                            plan.melds.iter().flat_map(|m| m.tiles()).for_each(|t| used[t as usize] += 1);
                            assert_eq!(used, hand, "seed {seed}: the plan must account for every tile exactly once");
                            compared += 1;
                        }
                    }
                }
            }
        }
        assert!(compared > 20_000);
    }
}

/// The remembered-hand bounds must hold between any two hands, however different.
#[test]
fn neighbour_bounds_never_contradict_the_search() {
    let rules = Rules::default();
    let mut sv = Solver::new(&rules);
    let open = Params {
        mode: Mode::Open,
        cover_w: 0,
        must: None,
        allow_melds: true,
        pair_dump: false,
        pair_cap: 0,
        joker_runs: true,
    };
    let cover = Params {
        mode: Mode::Penalty,
        cover_w: 1000,
        ..open
    };
    let mut rng = Rng::new(5);
    let mut tight = 0;
    for seed in 0..600u64 {
        let g = Game::deal(&rules, &mut Rng::new(seed), 0);
        let mut hand = g.players[0].hand;
        for step in 0..12 {
            let value = sv.run(&hand, open, None).map_or(0, |p| p.open_value(&rules));
            let left = sv.run(&hand, cover, None).map_or(0, |p| p.rest.len());
            assert!(sv.value_at_most(&hand) >= value, "seed {seed} step {step}: value bound too low");
            assert!(
                sv.left_over_at_least(&hand) as usize <= left,
                "seed {seed} step {step}: left-over bound too high"
            );
            tight += (sv.left_over_at_least(&hand) >= 2) as u32;
            sv.note_value(&hand, value);
            sv.note_left_over(&hand, left);
            // Wander to a neighbouring hand: drop a tile or two, pick up a tile or two.
            for _ in 0..1 + rng.below(2) {
                let have = tiles_of(&hand);
                hand[have[rng.below(have.len())] as usize] -= 1;
                let t = loop {
                    let t = rng.below(NIDS);
                    if hand[t] < 2 {
                        break t;
                    }
                };
                hand[t] += 1;
            }
        }
    }
    assert!(tight > 1000, "the bound should actually bite, got {tight}");
}

#[test]
fn pair_stats_agree_with_the_full_pair_plan() {
    use okey_engine::pairs::pair_stats;
    for (two, single) in [(false, true), (true, true), (false, false)] {
        let rules = Rules {
            two_jokers_pair: two,
            joker_pairs_with_single: single,
            ..Rules::default()
        };
        for seed in 0..2000u64 {
            let g = Game::deal(&rules, &mut Rng::new(seed), 0);
            for p in 0..4 {
                let hand = g.players[p].hand;
                for gost in [None, Some(g.gosterge)] {
                    let plan = pair_plan(&hand, &rules, gost);
                    assert_eq!(pair_stats(&hand, &rules, gost), (plan.pairs.len(), plan.rest.len()), "seed {seed}");
                }
            }
        }
    }
}

#[test]
fn opponent_reads_are_exact_for_a_known_hand_and_sum_to_one() {
    use okey_engine::reads::{Now, read_opponents};
    let mut view = sample_view(Phase::AfterDraw);
    let g = Game::deal(&view.rules, &mut Rng::new(42), 0);
    view.players[1].known = tiles_of(&g.players[1].hand);
    let mut sv = Solver::new(&view.rules);
    let exact = g
        .open_search(&mut sv, &g.players[1].hand, None)
        .map_or(0, |p| p.open_value(&view.rules));
    let now = Now {
        seat: 0,
        drawn: true,
        taken: None,
    };
    let reads = read_opponents(&view, now, 200, 3);
    assert_eq!(reads.len(), 3);
    assert_eq!(reads[0].per, [exact; 3]);
    for r in &reads {
        let bands: f32 = r.per_dist.iter().sum();
        let pairs: f32 = r.pairs.iter().sum();
        assert!((bands - 1.0).abs() < 1e-3 && (pairs - 1.0).abs() < 1e-3, "{bands} {pairs}");
        assert!(r.per[0] <= r.per[1] && r.per[1] <= r.per[2]);
        assert!(r.p_either >= r.p_melds.max(r.p_pairs) - 1e-6);
        assert!(r.p_open_soon <= r.p_open_end + 1e-6 && r.p_open_end <= 1.0 + 1e-6);
        assert!(r.p_finish_soon <= r.p_finish_end + 1e-6);
        assert!(r.p_open_end > 0.0, "with a full deck left somebody's opening must show up");
    }
}

#[test]
fn an_okey_entered_first_or_last_stands_at_that_end() {
    let r = Rules::default();
    let run = |s: &str| {
        classify(
            &s.split_whitespace()
                .map(|x| if x == "OK" { JOKER } else { t(x) })
                .collect::<Vec<_>>(),
            &r,
        )
        .unwrap()
    };
    let low = Shape::Run {
        color: 3,
        lo: 7,
        hi: 11,
        jokers: 1 << 7,
    };
    let high = Shape::Run {
        color: 3,
        lo: 8,
        hi: 12,
        jokers: 1 << 12,
    };
    assert_eq!(run("OK Si8 Si9 Si10 Si11"), low);
    assert_eq!(run("Si11 Si10 Si9 Si8 OK"), low);
    assert_eq!(run("Si8 Si9 Si10 Si11 OK"), high);
    assert_eq!(run("OK Si11 Si10 Si9 Si8"), high);
    assert_eq!(run("Si9 Si8 Si10 Si11 OK"), high, "out of order falls back to the top end");
    assert_eq!(
        run("OK K1 K2"),
        Shape::Run {
            color: 0,
            lo: 1,
            hi: 3,
            jokers: 1 << 3
        },
        "nothing below 1"
    );
    assert_eq!(
        run("K12 K13 OK"),
        Shape::Run {
            color: 0,
            lo: 11,
            hi: 13,
            jokers: 1 << 11
        },
        "nothing above 13"
    );
}
