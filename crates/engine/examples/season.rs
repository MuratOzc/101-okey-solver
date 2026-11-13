//! Whole matches against finisher bots, on the same deals, with the hero playing for points, for the
//! match in the last two hands only, or for the match throughout.
//! Usage: season <matches> <network> [hands per match] [hands already played] [points behind everyone] [iterations] [last hands left unplayed]
//! With hands left unplayed, a match counts as the chance of winning it from there (the same estimate the analyzer plays for).

use okey_engine::agent::*;
use okey_engine::analysis::Options;
use okey_engine::game::Game;
use okey_engine::meld::Solver;
use okey_engine::net::Net;
use okey_engine::policy::Policy;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use okey_engine::stakes::Stakes;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const MODES: [&str; 3] = ["points", "match, last two hands", "match, every hand"];

fn spread(v: &[f64]) -> (f64, f64) {
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    (mean, (var / n).sqrt())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let num = |i: usize, d: i64| args.get(i).and_then(|s| s.parse::<i64>().ok()).unwrap_or(d);
    let matches = num(1, 200) as u64;
    let mut net = Net::from_file(args.get(2).expect("network")).expect("network");
    net.discards = 5;
    let net = Arc::new(net);
    let (hands, played, behind, iterations) = (num(3, 11) as u64, num(4, 0) as u64, num(5, 0) as i32, num(6, 60) as usize);
    let unplayed = num(7, 0) as u64;
    let rules = Rules::default();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(2).max(1));
    let next = AtomicU64::new(0);
    let start = Instant::now();
    let rows: Vec<[(f64, f64); 3]> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                let (next, rules, net) = (&next, &rules, &net);
                scope.spawn(move || {
                    let mut sv = Solver::new(rules);
                    let mut out = Vec::new();
                    loop {
                        let m = next.fetch_add(1, Ordering::Relaxed);
                        if m >= matches {
                            break;
                        }
                        let mut row = [(0.0, 0.0); 3];
                        for (mode, slot) in row.iter_mut().enumerate() {
                            if mode == 1 && unplayed >= 2 {
                                continue;
                            }
                            let mut tot = [behind, 0, 0, 0];
                            for h in played..hands - unplayed {
                                let seed = 0x5EA5 ^ (m * 64 + h).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                                let mut g = Game::deal(rules, &mut Rng::new(seed), ((m + h) % 4) as u8);
                                let left = (hands - 1 - h) as u8;
                                if mode == 2 || (mode == 1 && left <= 1) {
                                    g.stakes = Some(Stakes {
                                        hands_left: left,
                                        behind: [tot[0] - tot[1], tot[0] - tot[2], tot[0] - tot[3]],
                                    });
                                }
                                let mut hero = HelpedAgent {
                                    label: String::new(),
                                    opts: Options::default(),
                                    iterations,
                                    seed: m * 64 + h,
                                    help: Help {
                                        peek: [false; 4],
                                        model: Some((net.clone(), 0.0, true)),
                                    },
                                };
                                let mut bots: Vec<BotAgent> = (0..3)
                                    .map(|_| BotAgent {
                                        label: String::new(),
                                        policy: Policy::FINISHER,
                                    })
                                    .collect();
                                let [a, b, c] = &mut bots[..] else { unreachable!() };
                                let s = play_hand(&mut g, &mut sv, &mut [&mut hero, a, b, c]);
                                for (t, x) in tot.iter_mut().zip(s) {
                                    *t += x;
                                }
                            }
                            let best = tot[1].min(tot[2]).min(tot[3]);
                            let win = if unplayed > 0 {
                                let from_here = Stakes {
                                    hands_left: unplayed as u8,
                                    behind: [tot[0] - tot[1], tot[0] - tot[2], tot[0] - tot[3]],
                                };
                                1.0 - from_here.loss(&[0; 4], false)
                            } else if tot[0] < best {
                                1.0
                            } else if tot[0] == best {
                                0.5
                            } else {
                                0.0
                            };
                            *slot = (win, (tot[1] + tot[2] + tot[3]) as f64 / 3.0 - tot[0] as f64);
                        }
                        out.push(row);
                    }
                    out
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().unwrap()).collect()
    });
    println!(
        "{matches} matches of {hands} hands, starting after hand {played} with the hero {behind} behind everyone, last {unplayed} hands not played; {iterations} deals a decision, {:.0}s\n",
        start.elapsed().as_secs_f64()
    );
    println!(
        "{:<26}{:>16}{:>22}{:>24}",
        "hero plays for", "matches won", "against 'points'", "lead at the end"
    );
    for (k, name) in MODES.iter().enumerate() {
        let wins: Vec<f64> = rows.iter().map(|r| r[k].0 * 100.0).collect();
        let edge: Vec<f64> = rows.iter().map(|r| (r[k].0 - r[0].0) * 100.0).collect();
        let lead: Vec<f64> = rows.iter().map(|r| r[k].1).collect();
        let ((w, wse), (e, ese), (l, lse)) = (spread(&wins), spread(&edge), spread(&lead));
        println!("{name:<26}{w:>8.1}% ± {wse:<4.1}{e:>+13.1} ± {ese:<4.1}{l:>+15.0} ± {lse:<4.0}");
    }
}
