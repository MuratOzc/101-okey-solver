use okey_engine::analysis::*;
use okey_engine::game::*;
use okey_engine::meld::Solver;
use okey_engine::pairs::pair_stats;
use okey_engine::policy::Policy;
use okey_engine::reads::read_with;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use std::time::Instant;

fn main() {
    let rules = Rules::default();
    let mut sv = Solver::new(&rules);
    let (mut n, mut inside, mut med_err, mut pair_hit, mut brier, mut brier0, mut ready, mut pred) =
        (0, 0, 0f64, 0f64, 0f64, 0f64, 0f64, 0f64);
    let mut t = 0f64;
    let mut kept = vec![];
    for seed in 0..3000u64 {
        let mut g = Game::deal(&rules, &mut Rng::new(seed), 1);
        g.policy = [if std::env::var("FIN").is_ok() {
            Policy::FINISHER
        } else {
            Policy::TUNED
        }; 4];
        let stop = 1 + (seed % 7) as u32;
        let mut turns = 0;
        let mut over = g.starter_turn(&mut sv).is_some();
        while !over && !(g.current == 0 && turns >= stop * 4) {
            over = g.bot_turn(&mut sv).is_some();
            turns += 1;
        }
        if over {
            continue;
        }
        let view = public_view(&g, Phase::BeforeDraw, None);
        let t0 = Instant::now();
        let infer = std::env::args().nth(1).is_none();
        let reads = read_with(
            &view,
            300,
            seed,
            &Options {
                infer,
                ..Options::default()
            },
        );
        t += t0.elapsed().as_secs_f64();
        for r in reads {
            let p = &g.players[r.seat as usize];
            if p.opened != Opened::No {
                continue;
            }
            let v = g.open_search(&mut sv, &p.hand, None).map_or(0, |pl| pl.open_value(&rules));
            let np = pair_stats(&p.hand, &rules, None).0.min(7);
            let truth = (v >= r.meld_req || np >= r.pair_req as usize) as i32 as f64;
            n += 1;
            inside += (r.per[0] <= v && v <= r.per[2]) as i32;
            med_err += (r.per[1] - v).abs() as f64;
            pair_hit += r.pairs[np] as f64;
            brier += (r.p_either as f64 - truth).powi(2);
            ready += truth;
            pred += r.p_either as f64;
            kept.push(truth);
        }
    }
    let base = ready / n as f64;
    for &x in &kept {
        brier0 += (base - x).powi(2);
    }
    let n = n as f64;
    println!(
        "{n} unopened opponents, {:.1} ms per position (3 reads x 300 deals)",
        1000.0 * t / (n / 2.5)
    );
    println!(
        "true per inside 10-90%: {:.1}%   median off by {:.1}",
        100.0 * inside as f64 / n,
        med_err / n
    );
    println!("prob given to the true pair count: {:.1}%", 100.0 * pair_hit / n);
    println!(
        "ready now: truth {:.1}%  predicted {:.1}%   brier {:.4} vs base-rate {:.4}",
        100.0 * base,
        100.0 * pred / n,
        brier / n,
        brier0 / n
    );
}
