//! A rated league. Every hand seats four different players at random; each pair of them is one
//! comparison (who scored better), and Elo-scale ratings are fitted to all comparisons.
//! Two ratings are reported: per single hand, and per whole game of several hands.
//! Usage: league [hands] [search-iterations-per-decision] [hands-per-game]

use okey_engine::agent::*;
use okey_engine::analysis::Options;
use okey_engine::game::Game;
use okey_engine::meld::Solver;
use okey_engine::policy::Policy;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

fn roster(iterations: usize, seed: u64) -> Vec<Box<dyn Agent>> {
    let search = |label: &str, opts: Options| -> Box<dyn Agent> {
        Box::new(SearchAgent {
            label: label.to_string(),
            opts,
            iterations,
            seed,
        })
    };
    let base = Options {
        lookahead: false,
        ..Options::default()
    };
    vec![
        Box::new(BotAgent {
            label: "bot: basic".into(),
            policy: Policy::BASIC,
        }),
        Box::new(BotAgent {
            label: "bot: tuned".into(),
            policy: Policy::TUNED,
        }),
        search("search: default", base),
        search("search: + look-ahead", Options { lookahead: true, ..base }),
        Box::new(OracleAgent {
            label: "ORACLE (sees every hand; not a legal player)".into(),
            policy: Policy::TUNED,
            depth: 1,
        }),
    ]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let hands: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(400);
    let iterations: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let game_hands: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(11);
    let rules = Rules::default();
    let names: Vec<String> = roster(iterations, 0).iter().map(|a| a.name()).collect();
    let n = names.len();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let next = AtomicU64::new(0);
    let t0 = Instant::now();

    // Per thread: score differences for every ordered pair, score totals, hands played.
    type Part = (Vec<Vec<Vec<f32>>>, Vec<f64>, Vec<u32>);
    let parts: Vec<Part> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..threads)
            .map(|th| {
                let (rules, next) = (&rules, &next);
                sc.spawn(move || {
                    let mut players = roster(iterations, 1000 + th as u64);
                    let mut sv = Solver::new(rules);
                    let mut diffs = vec![vec![Vec::new(); n]; n];
                    let mut total = vec![0.0; n];
                    let mut played = vec![0u32; n];
                    loop {
                        let hand = next.fetch_add(1, Ordering::Relaxed);
                        if hand >= hands {
                            break (diffs, total, played);
                        }
                        let mut rng = Rng::new(9_000_000 + hand);
                        let mut order: Vec<usize> = (0..n).collect();
                        rng.shuffle(&mut order);
                        let seated = [order[0], order[1], order[2], order[3]];
                        let mut g = Game::deal(rules, &mut rng, (hand % 4) as u8);
                        let mut refs: Vec<(usize, &mut Box<dyn Agent>)> =
                            players.iter_mut().enumerate().filter(|(i, _)| seated.contains(i)).collect();
                        refs.sort_by_key(|(i, _)| seated.iter().position(|s| s == i));
                        let [a, b, c, d] = &mut refs[..] else { unreachable!() };
                        let mut seats: [&mut dyn Agent; 4] = [a.1.as_mut(), b.1.as_mut(), c.1.as_mut(), d.1.as_mut()];
                        let scores = play_hand(&mut g, &mut sv, &mut seats);
                        for x in 0..4 {
                            total[seated[x]] += scores[x] as f64;
                            played[seated[x]] += 1;
                            for y in 0..4 {
                                if x != y {
                                    diffs[seated[x]][seated[y]].push((scores[x] - scores[y]) as f32);
                                }
                            }
                        }
                    }
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let mut diffs = vec![vec![Vec::<f32>::new(); n]; n];
    let mut total = vec![0.0; n];
    let mut played = vec![0u32; n];
    for (d, t, p) in parts {
        for i in 0..n {
            total[i] += t[i];
            played[i] += p[i];
            for j in 0..n {
                diffs[i][j].extend(&d[i][j]);
            }
        }
    }

    // Lower score is better in 101, so a negative difference is a win.
    let mut hand_wins = vec![vec![0.0; n]; n];
    let mut game_wins = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            if i == j || diffs[i][j].is_empty() {
                continue;
            }
            let d = &diffs[i][j];
            hand_wins[i][j] = d
                .iter()
                .map(|&x| {
                    if x < 0.0 {
                        1.0
                    } else if x == 0.0 {
                        0.5
                    } else {
                        0.0
                    }
                })
                .sum();
            // Each game's worth of hands is one comparison, won with the chance the per-hand differences imply.
            game_wins[i][j] = d.len() as f64 / game_hands as f64 * game_win_chance(d, game_hands);
        }
    }
    let hand_elo = elo_from_pairs(&hand_wins, 1000.0);
    let game_elo = elo_from_pairs(&game_wins, 1000.0);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| game_elo[b].total_cmp(&game_elo[a]));
    println!(
        "{hands} hands, {iterations} search iterations per decision, {:.0}s",
        t0.elapsed().as_secs_f64()
    );
    println!("'bot: basic' is pinned at 1000. Hand Elo: who out-scored whom in one hand.");
    println!("Game Elo: who wins a whole game of {game_hands} hands (totals), from the same results.\n");
    println!(
        "{:<46} {:>8}  {:>8}  {:>9}  {:>6}",
        "player", "game Elo", "hand Elo", "avg score", "hands"
    );
    for i in order {
        println!(
            "{:<46} {:>8.0}  {:>8.0}  {:>+9.1}  {:>6}",
            names[i],
            game_elo[i],
            hand_elo[i],
            total[i] / played[i].max(1) as f64,
            played[i]
        );
    }
}
