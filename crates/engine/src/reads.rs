use crate::analysis::{Options, View};
use crate::game::Opened;
use crate::meld::{Solver, hand_penalty};
use crate::pairs::pair_stats;
use crate::rng::Rng;
use serde::{Deserialize, Serialize};

pub const MAX_PAIRS: usize = 7;

/// What one opponent's hidden hand probably holds, from many deals that fit what they have done.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Read {
    pub seat: u8,
    pub opened: Opened,
    pub deals: u32,
    /// Best meld opening the hand holds now: 10%, 50% and 90% points of the deals.
    pub per: [i32; 3],
    /// Share of deals per best opening total (index = total).
    pub per_dist: Vec<f32>,
    /// Share of deals with that many pairs; the last entry is `MAX_PAIRS` or more.
    pub pairs: Vec<f32>,
    pub meld_req: i32,
    pub pair_req: u8,
    pub p_melds: f32,
    pub p_pairs: f32,
    pub p_either: f32,
    /// What the hand would cost if the hand ended now (opened players): 10%, 50%, 90%.
    pub points: [i32; 3],
}

fn quantiles(v: &mut [i32]) -> [i32; 3] {
    if v.is_empty() {
        return [0; 3];
    }
    v.sort_unstable();
    let at = |q: f32| v[((v.len() - 1) as f32 * q).round() as usize];
    [at(0.1), at(0.5), at(0.9)]
}

/// Estimates for seats 1–3, each from `deals` deals of the hidden tiles drawn as the search draws them.
pub fn read_opponents(view: &View, deals: usize, seed: u64) -> Vec<Read> {
    read_with(view, deals, seed, &Options::default())
}

pub fn read_with(view: &View, deals: usize, seed: u64, opts: &Options) -> Vec<Read> {
    let mut rng = Rng::new(seed);
    let mut sv = Solver::new(&view.rules);

    let (meld_req, pair_req) = view.requirements();
    let mut pers = [vec![], vec![], vec![]];
    let mut pairs = [vec![], vec![], vec![]];
    let mut points = [vec![], vec![], vec![]];
    let mut either = [0u32; 3];
    for _ in 0..deals {
        let g = view.sample(&mut rng, &mut sv, opts);
        for o in 1..4usize {
            let p = &g.players[o];
            let v = g.open_search(&mut sv, &p.hand, None).map_or(0, |pl| pl.open_value(&view.rules));
            let gost = p.gosterge_bonus.then_some(view.gosterge);
            let n = pair_stats(&p.hand, &view.rules, gost).0;
            let pts = hand_penalty(&p.hand, &view.rules) * if p.opened == Opened::Pairs { 2 } else { 1 };
            if v >= meld_req || (view.rules.pairs_enabled && n >= pair_req as usize) {
                either[o - 1] += 1;
            }
            pers[o - 1].push(v);
            pairs[o - 1].push(n);
            points[o - 1].push(pts);
        }
    }
    let d = deals.max(1) as f32;
    (0..3)
        .map(|i| {
            let share = |k: usize| k as f32 / d;
            let top = pers[i].iter().copied().max().unwrap_or(0).max(meld_req);
            let mut dist = vec![0f32; top as usize + 1];
            pers[i].iter().for_each(|&v| dist[v as usize] += 1.0 / d);
            let mut pd = vec![0f32; MAX_PAIRS + 1];
            pairs[i].iter().for_each(|&n| pd[n.min(MAX_PAIRS)] += 1.0 / d);
            Read {
                seat: i as u8 + 1,
                opened: view.players[i + 1].opened,
                deals: deals as u32,
                per_dist: dist,
                p_melds: share(pers[i].iter().filter(|&&v| v >= meld_req).count()),
                p_pairs: if view.rules.pairs_enabled {
                    share(pairs[i].iter().filter(|&&n| n >= pair_req as usize).count())
                } else {
                    0.0
                },
                p_either: share(either[i] as usize),
                per: quantiles(&mut pers[i]),
                pairs: pd,
                points: quantiles(&mut points[i]),
                meld_req,
                pair_req,
            }
        })
        .collect()
}
