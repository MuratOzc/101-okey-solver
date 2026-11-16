//! How well the opponent estimates match the real hands, in positions from bot games.
//! `reads_check [off]`: off weighs every deal the same. FIN=1 uses bots that open at once.

use okey_engine::analysis::*;
use okey_engine::game::*;
use okey_engine::meld::Solver;
use okey_engine::pairs::pair_stats;
use okey_engine::policy::Policy;
use okey_engine::reads::read_with;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;

#[derive(Default)]
struct Bucket {
    n: f64,
    truth: f64,
    pred: f64,
    brier: f64,
    inside: f64,
}

fn main() {
    let rules = Rules::default();
    let mut sv = Solver::new(&rules);
    let infer = std::env::args().nth(1).is_none();
    let policy = if std::env::var("FIN").is_ok() {
        Policy::FINISHER
    } else {
        Policy::TUNED
    };
    let mut buckets = [Bucket::default(), Bucket::default()];
    let (mut okey_n, mut okey_brier, mut pts_inside, mut below, mut above) = (0f64, 0f64, 0f64, 0f64, 0f64);
    for seed in 0..3000u64 {
        let mut g = Game::deal(&rules, &mut Rng::new(seed), 1);
        g.policy = [policy; 4];
        let stop = 1 + (seed % 9) as u32;
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
        for r in read_with(&view, 300, seed, infer) {
            let p = &g.players[r.seat as usize];
            if p.opened != Opened::No {
                let has = (p.hand[52] > 0) as i32 as f64;
                let pts =
                    (0..52).map(|t| p.hand[t] as i32 * (t as i32 % 13 + 1)).sum::<i32>() * if p.opened == Opened::Pairs { 2 } else { 1 };
                okey_n += 1.0;
                okey_brier += (r.p_okey as f64 - has).powi(2);
                pts_inside += (r.points[0] <= pts && pts <= r.points[2]) as i32 as f64;
                below += (pts < r.points[0]) as i32 as f64;
                above += (pts > r.points[2]) as i32 as f64;
                continue;
            }
            let v = g.open_search(&mut sv, &p.hand, None).map_or(0, |pl| pl.open_value(&rules));
            let np = pair_stats(&p.hand, &rules, None).0;
            let truth = (v >= r.meld_req || np >= r.pair_req as usize) as i32 as f64;
            let b = &mut buckets[(view.deck_count <= 8) as usize];
            b.n += 1.0;
            b.truth += truth;
            b.pred += r.p_either as f64;
            b.brier += (r.p_either as f64 - truth).powi(2);
            b.inside += (r.per[0] <= v && v <= r.per[2]) as i32 as f64;
        }
    }
    for (name, b) in ["deck > 8", "deck <= 8"].iter().zip(&buckets) {
        let base = b.truth / b.n;
        println!(
            "{name}: {} unopened  per inside 10-90% {:.1}%  can open: truth {:.1}% predicted {:.1}%  brier {:.4} (base rate {:.4})",
            b.n,
            100.0 * b.inside / b.n,
            100.0 * base,
            100.0 * b.pred / b.n,
            b.brier / b.n,
            base * (1.0 - base)
        );
    }
    println!(
        "opened: {okey_n}  points inside 10-90% {:.1}% (below {:.1}%, above {:.1}%)  okey brier {:.4}",
        100.0 * pts_inside / okey_n,
        100.0 * below / okey_n,
        100.0 * above / okey_n,
        okey_brier / okey_n
    );
}
