//! How many distinct turn positions the exhaustive generator finds in real mid-game situations, and how long it takes.

use okey_engine::game::*;
use okey_engine::meld::Solver;
use okey_engine::moves::generate;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use okey_engine::tile::*;
use std::time::Instant;

fn main() {
    let rules = Rules::default();
    let mut sv = Solver::new(&rules);
    let cap = 20_000;
    let (mut counts, mut times, mut moves): (Vec<usize>, Vec<f64>, Vec<usize>) = (vec![], vec![], vec![]);
    let (mut truncated, mut opened_n, mut unopened_n) = (0, 0, 0);
    for seed in 0..600u64 {
        let mut g = Game::deal(&rules, &mut Rng::new(seed), 1);
        let mut ended = g.starter_turn(&mut sv).is_some();
        while !ended {
            if g.current == 0 {
                g.table.new_turn();
                let Some(t) = g.deck.pop() else { break };
                g.players[0].hand[t as usize] += 1;
                let t0 = Instant::now();
                let found = generate(&g, 0, None, cap);
                times.push(t0.elapsed().as_secs_f64() * 1000.0);
                counts.push(found.positions.len());
                truncated += found.truncated as usize;
                if g.players[0].opened == Opened::No {
                    unopened_n += 1
                } else {
                    opened_n += 1
                }
                let mut with_discards = 0;
                for acts in &found.positions {
                    let mut g2 = g.clone();
                    g2.play_acts(0, acts, None).expect("generated positions are legal");
                    with_discards += (0..NIDS).filter(|&i| g2.players[0].hand[i] > 0).count();
                }
                moves.push(with_discards);
                ended = g.bot_act(&mut sv, 0, None).is_some();
                g.current = 1;
            } else {
                ended = g.bot_turn(&mut sv).is_some();
            }
        }
    }
    let pct = |v: &mut Vec<f64>, q: f64| {
        v.sort_by(f64::total_cmp);
        v[((v.len() - 1) as f64 * q) as usize]
    };
    let mut c: Vec<f64> = counts.iter().map(|&x| x as f64).collect();
    let mut m: Vec<f64> = moves.iter().map(|&x| x as f64).collect();
    println!(
        "{} decision points ({unopened_n} unopened, {opened_n} opened), {truncated} hit the cap of {cap}",
        counts.len()
    );
    println!(
        "positions per turn : median {:.0}, 90% {:.0}, 99% {:.0}, max {:.0}",
        pct(&mut c, 0.5),
        pct(&mut c, 0.9),
        pct(&mut c, 0.99),
        pct(&mut c, 1.0)
    );
    println!(
        "moves incl. discard: median {:.0}, 90% {:.0}, 99% {:.0}, max {:.0}",
        pct(&mut m, 0.5),
        pct(&mut m, 0.9),
        pct(&mut m, 0.99),
        pct(&mut m, 1.0)
    );
    println!(
        "generation time ms : median {:.2}, 90% {:.2}, 99% {:.2}, max {:.1}",
        pct(&mut times, 0.5),
        pct(&mut times, 0.9),
        pct(&mut times, 0.99),
        pct(&mut times, 1.0)
    );
}
