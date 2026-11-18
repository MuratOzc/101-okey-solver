use okey_engine::analysis::*;
mod common;

use common::*;
use okey_engine::game::*;
use okey_engine::meld::Solver;
use okey_engine::net::Net;
use okey_engine::policy::Policy;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use std::sync::Arc;

fn midgame(rules: &Rules, sv: &mut Solver, seed: u64, turns: usize) -> Option<View> {
    let mut g = Game::deal(rules, &mut Rng::new(seed), 0);
    g.policy = [Policy::FINISHER; 4];
    if g.starter_turn(sv).is_some() {
        return None;
    }
    for _ in 0..turns {
        if g.bot_turn(sv).is_some() {
            return None;
        }
    }
    if g.current != 0 {
        return None;
    }
    g.table.new_turn();
    let t = g.deck.pop()?;
    g.players[0].hand[t as usize] += 1;
    Some(public_view(&g, Phase::AfterDraw, None))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let want: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(60);
    let long: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(12000);
    let rules = Rules::default();
    let mut net = Net::from_file("crates/app/model/value.bin").unwrap();
    net.discards = 5;
    let net = Arc::new(net);
    let mut sv = Solver::new(&rules);
    let views: Vec<View> = (0..5000u64)
        .filter_map(|s| midgame(&rules, &mut sv, s, 3 + 4 * (s % 6) as usize))
        .take(want)
        .collect();
    let rows: Vec<(usize, usize, u64, bool, f64, f64, String, String)> = parallel(&rules, views.len() as u64, |_, i| {
        let i = i as usize;
        let view = &views[i];
        let mut a = Analyzer::new(view.clone(), 1000 + i as u64).unwrap();
        a.use_model(net.clone());
        let mut ra = a.report();
        while !ra.settled && ra.iterations < 40000 {
            a.step(8);
            ra = a.report();
        }
        let mut b = Analyzer::new(view.clone(), 5000 + i as u64).unwrap();
        b.use_model(net.clone());
        b.step(long);
        let rb = b.report();
        let (ca, cb) = (&ra.candidates[0], &rb.candidates[0]);
        let same = ca.decision == cb.decision;
        let (mut d, mut se) = (0.0, 0.0);
        if !same {
            let mut ev = Analyzer::new(view.clone(), 9000 + i as u64).unwrap();
            ev.use_model(net.clone());
            ev.set_field(&Field {
                version: 0,
                depth: 0,
                cands: vec![ca.decision, cb.decision],
            });
            let diffs: Vec<f64> = (0..6000u64)
                .map(|k| ev.evaluate(77_000 + k))
                .map(|r| (r[0][0] - r[1][0]) as f64 / 3.0)
                .collect();
            let n = diffs.len() as f64;
            d = diffs.iter().sum::<f64>() / n;
            se = (diffs.iter().map(|x| (x - d) * (x - d)).sum::<f64>() / (n - 1.0) / n).sqrt();
        }

        (i, ra.positions, ra.iterations, same, d, se, ca.label.clone(), cb.label.clone())
    });
    let n = rows.len() as f64;
    let same = rows.iter().filter(|r| r.3).count();
    let loss: f64 = rows.iter().map(|r| r.4).sum::<f64>() / n;
    let deals: f64 = rows.iter().map(|r| r.2 as f64).sum::<f64>() / n;
    println!(
        "{} positions; stop-when-settled used {deals:.0} deals on average, the long run {long}",
        rows.len()
    );
    println!(
        "same move: {same} of {}; average loss from stopping early {loss:+.2} points per decision",
        rows.len()
    );
    let mut diff: Vec<_> = rows.iter().filter(|r| !r.3).collect();
    diff.sort_by(|a, b| b.4.total_cmp(&a.4));
    for r in diff {
        println!(
            "  #{:<3} {:>5} positions, settled after {:>5} deals: {:+6.1} ± {:.1}  early [{}]  long [{}]",
            r.0, r.1, r.2, r.4, r.5, r.6, r.7
        );
    }
}
