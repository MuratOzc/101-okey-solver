//! Last hand of a match, seat 0 trails every opponent by the same margin. How often does it still win,
//! with and without knowing the match situation? Same deals for both.
//! Usage: comeback [hands] [iterations-per-decision]

use okey_engine::agent::*;
use okey_engine::analysis::Options;
use okey_engine::game::Game;
use okey_engine::meld::Solver;
use okey_engine::policy::Policy;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use okey_engine::stakes::Stakes;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let hands: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(300);
    let iterations: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(120);
    let rules = Rules::default();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    println!("{hands} hands per margin, {iterations} iterations per decision; opponents are tuned bots\n");
    println!(
        "{:>22} {:>16} {:>16} {:>14} {:>14}",
        "trailing everyone by", "knows the match", "plays the hand", "avg score", "avg score"
    );
    for margin in [-200, 0, 100, 200, 300, 400] {
        let stakes = Stakes {
            hands_left: 0,
            behind: [margin; 3],
        };
        let next = AtomicU64::new(0);
        let t0 = Instant::now();
        let parts: Vec<[f64; 4]> = std::thread::scope(|sc| {
            let handles: Vec<_> = (0..threads)
                .map(|th| {
                    let (rules, next) = (&rules, &next);
                    sc.spawn(move || {
                        let mut sv = Solver::new(rules);
                        let mut sum = [0.0f64; 4];
                        loop {
                            let hand = next.fetch_add(1, Ordering::Relaxed);
                            if hand >= hands {
                                break sum;
                            }
                            for (k, aware) in [true, false].into_iter().enumerate() {
                                let mut g = Game::deal(rules, &mut Rng::new(40_000 + hand), (hand % 4) as u8);
                                g.stakes = aware.then_some(stakes);
                                let mut hero = SearchAgent {
                                    label: String::new(),
                                    opts: Options::default(),
                                    iterations,
                                    seed: 9 + th as u64,
                                };
                                let mut bots: Vec<BotAgent> = (0..3)
                                    .map(|_| BotAgent {
                                        label: String::new(),
                                        policy: Policy::TUNED,
                                    })
                                    .collect();
                                let [a, b, c] = &mut bots[..] else { unreachable!() };
                                let mut seats: [&mut dyn Agent; 4] = [&mut hero, a, b, c];
                                let s = play_hand(&mut g, &mut sv, &mut seats);
                                sum[k] += 1.0 - stakes.loss(&s, false);
                                sum[2 + k] += s[0] as f64;
                            }
                        }
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let total = parts
            .iter()
            .fold([0.0; 4], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2], a[3] + p[3]]);
        let n = hands as f64;
        println!(
            "{:>22} {:>15.1}% {:>15.1}% {:>+14.1} {:>+14.1}   ({:.0}s)",
            margin,
            100.0 * total[0] / n,
            100.0 * total[1] / n,
            total[2] / n,
            total[3] / n,
            t0.elapsed().as_secs_f64()
        );
    }
    println!("\nwin = ending the match ahead of all three; a tie counts half. The last two columns are seat 0's own score in the hand.");
}
