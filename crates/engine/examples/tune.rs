//! Tunes the simulated players' heuristics by self-play.
//! A mutated policy sits at seat 0 against three copies of the current champion on the same deals;
//! it replaces the champion only if it is clearly better, and is then re-checked on fresh deals.
//! Usage: tune [rounds] [mutations-per-round] [deals-per-test]

use okey_engine::game::*;
use okey_engine::meld::Solver;
use okey_engine::policy::{OpenStyle, Policy};
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

fn gap(s: &[i32; 4]) -> f64 {
    s[0] as f64 - (s[1] + s[2] + s[3]) as f64 / 3.0
}

fn play(rules: &Rules, sv: &mut Solver, seed: u64, hero: &Policy, others: &Policy) -> f64 {
    let mut g = Game::deal(rules, &mut Rng::new(seed), (seed % 4) as u8);
    g.policy = [*hero, *others, *others, *others];
    let out = g.starter_turn(sv).unwrap_or_else(|| g.play_out(sv));
    gap(&g.scores(&out))
}

/// Mean and standard error of (candidate − champion) seat-0 gap on the same deals. Negative is better.
fn compare(rules: &Rules, cand: &Policy, champ: &Policy, first_seed: u64, deals: u64) -> (f64, f64) {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let next = AtomicU64::new(0);
    let diffs: Vec<f64> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                sc.spawn(|| {
                    let mut sv = Solver::new(rules);
                    let mut v = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= deals {
                            break v;
                        }
                        let seed = first_seed + i;
                        v.push(play(rules, &mut sv, seed, cand, champ) - play(rules, &mut sv, seed, champ, champ));
                    }
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    let n = diffs.len() as f64;
    let m = diffs.iter().sum::<f64>() / n;
    let var = diffs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1.0);
    (m, (var / n).sqrt())
}

fn normal(rng: &mut Rng) -> f32 {
    let u = |rng: &mut Rng| (rng.below(1_000_000) as f32 + 0.5) / 1_000_000.0;
    (0..6).map(|_| u(rng)).sum::<f32>() - 3.0
}

const STYLES: [OpenStyle; 5] = [
    OpenStyle::MaxValue,
    OpenStyle::MaxCover,
    OpenStyle::Minimal,
    OpenStyle::SafeOkey,
    OpenStyle::NoOkey,
];

fn mutate(p: &Policy, rng: &mut Rng) -> Policy {
    let mut q = *p;
    for _ in 0..1 + rng.below(3) {
        let scale = (normal(rng) * 0.6).exp();
        let nudge = normal(rng) * 0.6;
        match rng.below(13) {
            0 => q.w_adj = (q.w_adj * scale).clamp(0.1, 40.0),
            1 => q.w_gap = (q.w_gap * scale).clamp(0.05, 40.0),
            2 => q.w_group = (q.w_group * scale).clamp(0.1, 40.0),
            3 => q.w_live = (q.w_live + nudge * 0.4).clamp(0.0, 4.0),
            4 => q.w_value_unopened = (q.w_value_unopened + nudge * 2.0).clamp(-10.0, 20.0),
            5 => q.w_value_opened = (q.w_value_opened + nudge * 2.0).clamp(-5.0, 30.0),
            6 => q.w_danger = (q.w_danger + nudge * 3.0).clamp(0.0, 40.0),
            7 => q.hold_deck_quiet = [99, 18, 16, 14, 12, 10, 8, 6, 4, 2][rng.below(10)],
            8 => q.hold_deck_active = [99, 18, 16, 14, 12, 10, 8, 6, 4, 2][rng.below(10)],
            9 => q.open_style = STYLES[rng.below(5)],
            10 => q.lay_after_open = !q.lay_after_open,
            11 => q.joker_in_runs = !q.joker_in_runs,
            _ => q.w_meld = (q.w_meld * scale).clamp(20.0, 2000.0),
        }
    }
    q
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rounds: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    let mutations: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(120);
    let deals: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(6000);
    let rules = Rules::default();
    let mut rng = Rng::new(2026);
    let mut champ = Policy::BASIC;
    let mut seed = 1_000_000u64;
    let t0 = Instant::now();
    for round in 0..rounds {
        let start = champ;
        let mut accepted = 0;
        for _ in 0..mutations {
            let cand = mutate(&champ, &mut rng);
            if cand == champ {
                continue;
            }
            let (d, se) = compare(&rules, &cand, &champ, seed, deals);
            seed += deals;
            if d < -2.5 * se {
                let (d2, se2) = compare(&rules, &cand, &champ, seed, deals);
                seed += deals;
                if d2 < -1.5 * se2 {
                    champ = cand;
                    accepted += 1;
                    println!(
                        "  round {round}: accepted ({d:+.2}±{se:.2}, recheck {d2:+.2}±{se2:.2})  {:.0}s",
                        t0.elapsed().as_secs_f64()
                    );
                }
            }
        }
        let (d, se) = compare(&rules, &champ, &start, seed, deals * 4);
        seed += deals * 4;
        println!("round {round}: {accepted} accepted; new champion vs round start: {d:+.2} ± {se:.2} points/hand");
        println!("{champ:#?}");
    }
    let (d, se) = compare(&rules, &champ, &Policy::BASIC, seed, deals * 4);
    println!("FINAL vs BASIC opponents (champion at seat 0 against three BASIC): {d:+.2} ± {se:.2}");
    let (d, se) = compare(&rules, &Policy::BASIC, &champ, seed + deals * 4, deals * 4);
    println!("BASIC at seat 0 against three champions, relative to a champion there: {d:+.2} ± {se:.2}");
}
