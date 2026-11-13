use okey_engine::agent::{Agent, BotAgent};
use okey_engine::analysis::{Analyzer, Options, Phase, public_view};
use okey_engine::features::{Feat, LAYOUT, Moment, NBITS, NBITS_PLAIN, NBITS_V1, NREAL, NREAL_PLAIN, WORDS, encode};
use okey_engine::game::Game;
use okey_engine::meld::Solver;
use okey_engine::model::ModelAgent;
use okey_engine::net::Net;
use okey_engine::policy::Policy;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use okey_engine::stakes::Stakes;
use okey_engine::tile::*;
use std::sync::Arc;

fn net_bytes(partners: Option<bool>, nbits: usize, nreal: usize, hidden: usize, weight: &mut dyn FnMut() -> f32) -> Vec<u8> {
    let mut b = b"OKNN".to_vec();
    let head = match partners {
        None => vec![1, nbits as u32, nreal as u32],
        Some(p) => vec![LAYOUT, nbits as u32, nreal as u32, p as u32],
    };
    for x in head.into_iter().chain([300f32.to_bits(), 2]) {
        b.extend(x.to_le_bytes());
    }
    for (inputs, outputs) in [(nbits + nreal, hidden), (hidden, 1)] {
        b.extend((inputs as u32).to_le_bytes());
        b.extend((outputs as u32).to_le_bytes());
        for _ in 0..inputs * outputs + outputs {
            b.extend(weight().to_le_bytes());
        }
    }
    b
}

fn random_net(seed: u64, partners: bool) -> Net {
    let mut rng = Rng::new(seed);
    let mut weight = || (rng.below(2001) as f32 - 1000.0) / 20_000.0;
    Net::from_bytes(&net_bytes(Some(partners), NBITS_PLAIN, NREAL_PLAIN, 16, &mut weight)).unwrap()
}

fn midgame(seed: u64, turns: usize) -> Option<Game> {
    let rules = Rules::default();
    let mut rng = Rng::new(seed);
    let mut sv = Solver::new(&rules);
    let mut g = Game::deal(&rules, &mut rng, (seed % 4) as u8);
    g.policy = [Policy::TUNED; 4];
    if g.starter_turn(&mut sv).is_some() {
        return None;
    }
    for _ in 0..turns {
        if g.bot_turn(&mut sv).is_some() {
            return None;
        }
    }
    Some(g)
}

#[test]
fn inputs_show_nothing_of_the_hidden_tiles() {
    let rules = Rules::default();
    let mut sv = Solver::new(&rules);
    let mut checked = 0;
    for seed in 0..120u64 {
        let Some(g) = midgame(seed, (seed % 30) as usize) else { continue };
        let me = g.current;
        let mut hidden: Vec<Tile> = g.deck.clone();
        for p in (0..4).filter(|&p| p != me as usize) {
            hidden.extend(tiles_of(&g.players[p].hand));
        }
        Rng::new(seed ^ 77).shuffle(&mut hidden);
        let mut other = g.clone();
        for p in (0..4).filter(|&p| p != me as usize) {
            let n = total(&g.players[p].hand) as usize;
            other.players[p].hand = counts_from(&hidden.split_off(hidden.len() - n));
        }
        other.deck = hidden;
        for moment in [Moment::Draw, Moment::After] {
            for full in [false, true] {
                assert_eq!(encode(&g, &mut sv, me, moment, full), encode(&other, &mut sv, me, moment, full));
            }
        }
        checked += 1;
    }
    assert!(checked > 60);
}

#[test]
fn a_network_is_evaluated_as_written() {
    let mut k = 0;
    let mut weight = || {
        k += 1;
        (k % 7) as f32 - 3.0
    };
    let net = Net::from_bytes(&net_bytes(None, NBITS_V1, NREAL_PLAIN, 2, &mut weight)).unwrap();
    let mut f = Feat {
        bits: [0; WORDS],
        real: [0; NREAL],
    };
    f.bits[0] = 0b101;
    f.real[1] = 255;
    let inputs = NBITS_V1 + NREAL_PLAIN;
    let w = |i: usize| ((i + 1) % 7) as f32 - 3.0;
    let hidden: Vec<f32> = (0..2)
        .map(|o| (w(o * inputs) + w(o * inputs + 2) + w(o * inputs + NBITS_V1 + 1) + w(2 * inputs + o)).max(0.0))
        .collect();
    let base = 2 * inputs + 2;
    let want = (hidden[0] * w(base) + hidden[1] * w(base + 1) + w(base + 2)) * 300.0;
    assert!((net.value(&f) - want).abs() < 1e-3, "{} vs {want}", net.value(&f));

    f.bits[NBITS_V1 / 64] |= 1 << (NBITS_V1 % 64);
    f.bits[WORDS - 1] |= 1 << ((NBITS - 1) % 64);
    f.real[NREAL - 1] = 200;
    assert!((net.value(&f) - want).abs() < 1e-3);

    assert!(Net::from_bytes(&net_bytes(None, NBITS_V1 + 1, NREAL_PLAIN, 2, &mut weight)).is_err());
    assert!(Net::from_bytes(b"OKNN").is_err());
    let full = Net::from_bytes(&net_bytes(Some(true), NBITS, NREAL, 2, &mut weight)).unwrap();
    assert!(full.full() && full.partners && !net.partners);
}

#[test]
fn the_model_player_never_loses_or_invents_a_tile() {
    let rules = Rules::default();
    let mut sv = Solver::new(&rules);
    let mut ended = 0;
    for seed in 0..150u64 {
        let mut model = ModelAgent {
            label: "model".into(),
            net: Arc::new(random_net(seed, false)),
        };
        let mut bot = BotAgent {
            label: "bot".into(),
            policy: Policy::TUNED,
        };
        let mut g = Game::deal(&rules, &mut Rng::new(seed), (seed % 4) as u8);
        let mut first = true;
        for _ in 0..400 {
            let seat = g.current;
            let end = if seat == 0 {
                model.turn(&mut g, &mut sv, first)
            } else {
                bot.turn(&mut g, &mut sv, first)
            };
            first = false;
            assert_eq!(g.tile_count(), 106, "seed {seed}");
            if end.is_some() {
                ended += 1;
                break;
            }
            assert_eq!(g.current, (seat + 1) % 4);
            assert!(g.hand_len(seat) > 0, "seed {seed}: seat {seat} emptied its hand without finishing");
        }
    }
    assert_eq!(ended, 150);
}

#[test]
fn the_search_uses_the_network_only_where_it_was_trained() {
    let g = (0..50).find_map(|seed| midgame(seed, 8).filter(|g| g.current == 0)).unwrap();
    let uses = |partners: bool, change: &dyn Fn(&mut okey_engine::analysis::View)| {
        let mut view = public_view(&g, Phase::BeforeDraw, None);
        view.phase = Phase::AfterDraw;
        view.hand.push(g.deck[0]);
        view.deck_count -= 1;
        change(&mut view);
        let mut a = Analyzer::with_options(view, 1, Options::default()).unwrap();
        a.use_model(Arc::new(random_net(3, partners)), 0.0, true);
        a.step(4);
        a.report().model
    };
    assert!(uses(false, &|_| {}));
    assert!(!uses(false, &|v| v.rules.partners = true));
    assert!(uses(true, &|v| v.rules.partners = true));
    assert!(!uses(true, &|_| {}));
    assert!(!uses(false, &|v| v.rules.katlamali = true));
    let stakes = |hands_left| {
        move |v: &mut okey_engine::analysis::View| {
            v.stakes = Some(Stakes {
                hands_left,
                behind: [50; 3],
            })
        }
    };
    assert!(
        uses(false, &stakes(5)),
        "playing for the match, the network still plays my later turns"
    );
    assert!(!uses(false, &stakes(1)), "but not in the last two hands");
}

#[test]
fn only_deals_played_to_the_end_count_toward_how_the_hand_ends() {
    let g = (0..50).find_map(|seed| midgame(seed, 4).filter(|g| g.current == 0)).unwrap();
    let report = |horizon: u8, stakes: Option<Stakes>| {
        let mut view = public_view(&g, Phase::BeforeDraw, None);
        view.phase = Phase::AfterDraw;
        view.hand.push(g.deck[0]);
        view.deck_count -= 1;
        view.stakes = stakes;
        let mut a = Analyzer::with_options(
            view,
            1,
            Options {
                horizon,
                ..Options::default()
            },
        )
        .unwrap();
        a.use_model(Arc::new(random_net(3, false)), 0.0, true);
        a.step(60);
        a.report()
    };
    let cut = report(1, None);
    let best = &cut.candidates[0];
    assert!(
        best.whole > 0 && best.whole < best.n,
        "{} of {} deals went to the end",
        best.whole,
        best.n
    );
    assert!(
        cut.candidates
            .iter()
            .all(|c| c.p_finish <= 1.0 && c.p_last_draw.iter().sum::<f64>() <= 1.0 + 1e-9)
    );
    let full = report(0, None);
    assert!(full.candidates.iter().all(|c| c.whole == c.n));
    let behind = Stakes {
        hands_left: 5,
        behind: [50; 3],
    };
    let played_for_the_match = report(1, Some(behind));
    assert!(
        played_for_the_match.candidates.iter().all(|c| c.whole == c.n),
        "a match chance needs real scores"
    );
}

#[test]
fn a_position_valued_from_a_near_one_gets_the_same_value() {
    let rules = Rules::default();
    let mut sv = Solver::new(&rules);
    let net = random_net(11, false);
    let mut worst = 0f32;
    for seed in 0..40 {
        let Some(g) = midgame(seed, 6 + seed as usize % 9) else { continue };
        let me = g.current;
        let base = encode(&g, &mut sv, me, Moment::After, net.full());
        let near = net.near(&base);
        let hand = g.players[me as usize].hand;
        for t in (0..NIDS as Tile).filter(|&t| hand[t as usize] > 0) {
            let mut a = g.clone();
            a.discard(me, t);
            let f = encode(&a, &mut sv, me, Moment::After, net.full());
            worst = worst.max((net.value_near(&near, &f) - net.value(&f)).abs());
        }
    }
    assert!(worst < 0.01, "off by {worst}");
}

#[test]
fn the_player_among_all_moves_never_loses_or_invents_a_tile() {
    let rules = Rules::default();
    let mut sv = Solver::new(&rules);
    let net = random_net(5, false);
    for seed in 0..60u64 {
        let mut g = Game::deal(&rules, &mut Rng::new(seed), (seed % 4) as u8);
        let mut first = true;
        let mut ended = false;
        for _ in 0..400 {
            ended = okey_engine::model::model_turn_all(&mut g, &mut sv, &net, first).is_some();
            first = false;
            assert_eq!(g.tile_count(), 106, "seed {seed}");
            if ended {
                break;
            }
        }
        assert!(ended, "seed {seed}: the hand must end");
    }
}
