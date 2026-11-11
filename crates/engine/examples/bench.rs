//! Speed of the two things everything else is built on: full bot hands, and analysis of one position.

use okey_engine::analysis::*;
use okey_engine::game::*;
use okey_engine::meld::Solver;
use okey_engine::policy::Policy;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use std::time::Instant;

fn main() {
    let rules = Rules::default();
    let mut sv = Solver::new(&rules);
    for (name, policy) in [("basic", Policy::BASIC), ("tuned", Policy::TUNED)] {
        let n = 3000u64;
        let t0 = Instant::now();
        let mut sum = 0i64;
        for seed in 0..n {
            let mut g = Game::deal(&rules, &mut Rng::new(seed), (seed % 4) as u8);
            g.policy = [policy; 4];
            let o = g.starter_turn(&mut sv).unwrap_or_else(|| g.play_out(&mut sv));
            sum += g.scores(&o).iter().map(|&x| x as i64).sum::<i64>();
        }
        let dt = t0.elapsed().as_secs_f64();
        println!("{name} bots: {:.0} hands/s (checksum {sum})", n as f64 / dt);
    }

    let mut total = 0.0;
    let mut rollouts = 0u64;
    for seed in 0..12u64 {
        let mut g = Game::deal(&rules, &mut Rng::new(100 + seed), 0);
        g.table.new_turn();
        let view = public_view(&g, Phase::AfterDraw, None);
        let t0 = Instant::now();
        let mut a = Analyzer::new(view, seed).unwrap();
        a.step(150);
        total += t0.elapsed().as_secs_f64();
        rollouts += a.report().candidates.iter().map(|c| c.n as u64).sum::<u64>();
    }
    println!(
        "analysis: 12 opening positions × 150 deals in {total:.2}s ({:.0} listed rollouts/s)",
        rollouts as f64 / total
    );
}
