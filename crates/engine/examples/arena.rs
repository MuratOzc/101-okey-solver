//! Measures playing strength. Seat 0 plays the same deals under several configurations
//! (a plain bot, and the analyzer with different options), against weak and strong opponents.
//! Usage: arena [hands] [iterations-per-decision]

use okey_engine::analysis::*;
use okey_engine::game::*;
use okey_engine::meld::Solver;
use okey_engine::policy::Policy;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

fn gap(s: &[i32; 4]) -> f64 {
    s[0] as f64 - (s[1] + s[2] + s[3]) as f64 / 3.0
}

fn decide_move(
    g: &Game,
    phase: Phase,
    taken: Option<u8>,
    iters: usize,
    seed: u64,
    opts: &Options,
) -> (Option<Decision>, Option<okey_engine::moves::Move>) {
    let Ok(mut a) = Analyzer::with_options(public_view(g, phase, taken), seed, *opts) else {
        return (None, None);
    };
    // Screening spends its samples on the wide field; keep going until discards have been compared too.
    let mut done = 0;
    loop {
        a.step(25);
        done += 25;
        let r = a.report();
        // With look-ahead on, keep going until the deep stage has had its share of samples too.
        let deep_ready = !opts.lookahead || (r.deep && r.candidates.first().is_some_and(|c| c.n >= iters / 2)) || done >= iters * 3;
        if done >= iters && deep_ready && (!r.screening || done >= iters * 8) {
            return r.candidates.first().map_or((None, None), |c| (Some(c.decision), c.mv.clone()));
        }
    }
}

fn decide(g: &Game, phase: Phase, taken: Option<u8>, iters: usize, seed: u64, opts: &Options) -> Option<Decision> {
    decide_move(g, phase, taken, iters, seed, opts).0
}

/// Seat 0 follows the analyzer; the other seats play `opp`.
fn hero_hand(rules: &Rules, seed: u64, iters: usize, opts: &Options, opp: &Policy) -> [i32; 4] {
    let mut sv = Solver::new(rules);
    let mut g = Game::deal(rules, &mut Rng::new(seed), (seed % 4) as u8);
    g.policy = [opts.policy, *opp, *opp, *opp];
    let mut first = true;
    let out = loop {
        let p = g.current;
        if p != 0 {
            let r = if first { g.starter_turn(&mut sv) } else { g.bot_turn(&mut sv) };
            first = false;
            if let Some(o) = r {
                break o;
            }
            continue;
        }
        g.table.new_turn();
        let mut taken = None;
        if !first {
            match decide(&g, Phase::BeforeDraw, None, iters, seed, opts) {
                Some(Decision::Take) => taken = Some(g.take_top(0)),
                _ => match g.deck.pop() {
                    Some(t) => g.players[0].hand[t as usize] += 1,
                    None => break Outcome::DeckOut,
                },
            }
        }
        first = false;
        let (picked, mv) = decide_move(&g, Phase::AfterDraw, taken, iters, seed ^ 0x55, opts);
        let res = match picked {
            Some(Decision::Play(_)) => {
                let mv = mv.expect("the leading candidate carries its move");
                let backup = g.clone();
                match g.play_move(0, &mv, taken) {
                    Ok(o) => o,
                    Err(()) => {
                        g = backup;
                        g.bot_act(&mut sv, 0, taken)
                    }
                }
            }
            Some(Decision::Act { choice, discard }) => {
                let backup = g.clone();
                match g.apply_choice(&mut sv, 0, choice, taken) {
                    Ok(Some(o)) => Some(o),
                    Ok(None) => {
                        let t = discard
                            .filter(|&t| g.players[0].hand[t as usize] > 0)
                            .unwrap_or_else(|| g.bot_discard(&mut sv, 0));
                        g.discard(0, t);
                        None
                    }
                    Err(()) => {
                        g = backup;
                        g.bot_act(&mut sv, 0, taken)
                    }
                }
            }
            _ => g.bot_act(&mut sv, 0, taken),
        };
        if let Some(o) = res {
            break o;
        }
        g.current = 1;
    };
    g.scores(&out)
}

fn bot_hand(rules: &Rules, seed: u64, hero: &Policy, opp: &Policy) -> [i32; 4] {
    let mut sv = Solver::new(rules);
    let mut g = Game::deal(rules, &mut Rng::new(seed), (seed % 4) as u8);
    g.policy = [*hero, *opp, *opp, *opp];
    let out = g.starter_turn(&mut sv).unwrap_or_else(|| g.play_out(&mut sv));
    g.scores(&out)
}

enum Hero {
    Bot(Policy),
    Analyzer(Options),
}

fn stats(d: &[f64]) -> (f64, f64) {
    let n = d.len() as f64;
    let m = d.iter().sum::<f64>() / n;
    (m, (d.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1.0) / n).sqrt())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let hands: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(300);
    let iters: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100);
    let rules = Rules::default();
    let heroes = [
        ("plain bot (BASIC)", Hero::Bot(Policy::BASIC)),
        ("plain bot (TUNED)", Hero::Bot(Policy::TUNED)),
        (
            "analyzer v1: one plan, BASIC model",
            Hero::Analyzer(Options {
                rich_moves: false,
                policy: Policy::BASIC,
                max_discards: 10,
                infer: false,
                exhaustive: false,
                lookahead: false,
            }),
        ),
        (
            "analyzer v6: every legal move + inference",
            Hero::Analyzer(Options {
                rich_moves: true,
                policy: Policy::TUNED,
                max_discards: 12,
                infer: true,
                exhaustive: true,
                lookahead: false,
            }),
        ),
        (
            "analyzer v7: v6 + look-ahead",
            Hero::Analyzer(Options {
                rich_moves: true,
                policy: Policy::TUNED,
                max_discards: 12,
                infer: true,
                exhaustive: true,
                lookahead: true,
            }),
        ),
    ];
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    println!("{hands} hands, {iters} iterations per decision, {threads} threads");
    println!("gap = my score − opponents' average; lower is better\n");
    for (opp_name, opp) in [("BASIC", Policy::BASIC), ("TUNED", Policy::TUNED)] {
        let t0 = Instant::now();
        let next = AtomicU64::new(0);
        let rows: Vec<Vec<f64>> = std::thread::scope(|sc| {
            let handles: Vec<_> = (0..threads)
                .map(|_| {
                    sc.spawn(|| {
                        let mut v = Vec::new();
                        loop {
                            let seed = next.fetch_add(1, Ordering::Relaxed);
                            if seed >= hands {
                                break v;
                            }
                            let seed = seed + 5_000_000;
                            v.push(
                                heroes
                                    .iter()
                                    .map(|(_, h)| match h {
                                        Hero::Bot(p) => gap(&bot_hand(&rules, seed, p, &opp)),
                                        Hero::Analyzer(o) => gap(&hero_hand(&rules, seed, iters, o, &opp)),
                                    })
                                    .collect::<Vec<f64>>(),
                            );
                        }
                    })
                })
                .collect();
            handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
        });
        println!("against three {opp_name} opponents ({:.0}s):", t0.elapsed().as_secs_f64());
        let col = |i: usize| rows.iter().map(|r| r[i]).collect::<Vec<f64>>();
        let base = col(0);
        for (i, (name, _)) in heroes.iter().enumerate() {
            let c = col(i);
            let (m, se) = stats(&c);
            let diff: Vec<f64> = c.iter().zip(&base).map(|(a, b)| b - a).collect();
            let (dm, dse) = stats(&diff);
            println!("  {name:<40} gap {m:+7.1} ±{se:4.1}   better than plain BASIC bot by {dm:+6.1} ±{dse:4.1}");
        }
        println!();
    }
}
