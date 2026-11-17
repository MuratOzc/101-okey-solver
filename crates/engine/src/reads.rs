use crate::analysis::View;
use crate::game::{Game, Opened, Outcome};
use crate::meld::{Solver, hand_penalty};
use crate::pairs::pair_stats;
use crate::policy::Policy;
use crate::rng::Rng;
use crate::tile::{Counts, JOKER, NIDS, Tile, counts_from};
use serde::{Deserialize, Serialize};

pub const MAX_PAIRS: usize = 7;

/// What one opponent's hidden hand probably holds, from many deals weighted by how well they fit
/// what that player has done.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Read {
    pub seat: u8,
    pub opened: Opened,
    pub deals: u32,
    /// Best meld opening the hand holds now: 10%, 50% and 90% points.
    pub per: [i32; 3],
    /// Share per best opening total (index = total).
    pub per_dist: Vec<f32>,
    /// Share per pair count; the last entry is `MAX_PAIRS` or more.
    pub pairs: Vec<f32>,
    pub meld_req: i32,
    pub pair_req: u8,
    pub p_melds: f32,
    pub p_pairs: f32,
    pub p_either: f32,
    /// Tile points left in hand, okeys not counted (doubled for a pair opener): 10%, 50%, 90%.
    pub points: [i32; 3],
    /// Chance the hand holds at least one okey.
    pub p_okey: f32,
    /// Chance this seat has opened before my next turn begins, and by the end of the hand
    /// (counting an opening already down).
    #[serde(default)]
    pub p_open_soon: f32,
    #[serde(default)]
    pub p_open_end: f32,
    /// Chance this seat finishes the hand before my next turn begins, and at all.
    #[serde(default)]
    pub p_finish_soon: f32,
    #[serde(default)]
    pub p_finish_end: f32,
}

/// Whose turn it is and how far it has got, for playing the rest of the hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Now {
    pub seat: u8,
    pub drawn: bool,
    /// The tile the player on turn took from the side, which must be used.
    pub taken: Option<Tile>,
}

/// Draws each player has made so far, about: the deck is shared out evenly between the seats.
fn draws_so_far(view: &View) -> usize {
    let dealt = view.rules.hand_size as usize * 4 + view.rules.starter_extra as usize;
    let first_deck = 106usize.saturating_sub(dealt + 1);
    first_deck.saturating_sub(view.deck_count as usize).div_ceil(4)
}

/// A random deal gives an unopened opponent a hand as loose as the deck, but a real one has been
/// built: every turn the player kept what fitted and threw what did not. Replay those turns with
/// tiles that could have come instead, from the deck or another opponent's unknown tiles.
fn build_hands(g: &mut Game, view: &View, sv: &mut Solver, rng: &mut Rng) {
    let known: Vec<Counts> = view.players.iter().map(|p| counts_from(&p.known)).collect();
    let free = |g: &Game, q: usize, t: usize| g.players[q].hand[t].saturating_sub(known[q][t]);
    // Half the turns: replaying all of them made openings likelier than in bot play (50% against 47%).
    for _ in 0..draws_so_far(view).div_ceil(2) {
        for o in 1..4usize {
            if g.players[o].opened != Opened::No {
                continue;
            }
            let mut src: Vec<(usize, Tile)> = g.deck.iter().map(|&t| (0, t)).collect();
            for q in (1..4).filter(|&q| q != o) {
                for t in 0..NIDS {
                    for _ in 0..free(g, q, t) {
                        src.push((q, t as Tile));
                    }
                }
            }
            if src.is_empty() {
                return;
            }
            let (from, t) = src[rng.below(src.len())];
            g.players[o].hand[t as usize] += 1;
            let Some(d) = g
                .discard_scores(sv, o as u8)
                .into_iter()
                .filter(|&(d, _)| free(g, o, d as usize) > 0)
                .min_by_key(|&(_, s)| s)
                .map(|(d, _)| d)
            else {
                g.players[o].hand[t as usize] -= 1;
                continue;
            };
            g.players[o].hand[d as usize] -= 1;
            if d == t {
                continue;
            }
            if from == 0 {
                let i = g.deck.iter().position(|&x| x == t).expect("drawn from the deck");
                g.deck[i] = d;
            } else {
                g.players[from].hand[t as usize] -= 1;
                g.players[from].hand[d as usize] += 1;
            }
        }
    }
}

/// Plays the hand out with bots that open the moment they can. For seats 1–3: opened before my
/// next turn, opened by the end, finished before my next turn, finished.
fn play_ahead(g: &mut Game, sv: &mut Solver, now: Now) -> [[bool; 4]; 3] {
    g.policy = [Policy::FINISHER; 4];
    g.current = now.seat;
    let mut out = [[false; 4]; 3];
    let mut soon = true;
    let mut res = if now.drawn {
        g.bot_act(sv, now.seat, now.taken)
    } else {
        g.top = g.last[((now.seat + 3) % 4) as usize];
        g.bot_turn(sv)
    };
    loop {
        for o in 1..4usize {
            if g.players[o].opened != Opened::No {
                out[o - 1][1] = true;
                out[o - 1][0] |= soon;
            }
        }
        if let Some(o) = res {
            if let Outcome::Finished(f) = o
                && f.player != 0
            {
                out[f.player as usize - 1][3] = true;
                out[f.player as usize - 1][2] = soon;
            }
            return out;
        }
        if g.current == 0 {
            soon = false;
        }
        res = g.bot_turn(sv);
    }
}

/// How likely a player is to sit on an opening they could lay, by how many tiles are left in the deck.
/// Early on some players hold one back; near the end nobody does, since not opening costs 101.
pub fn holds_ready(deck: u8) -> f32 {
    const EARLY: f32 = 0.5;
    const LATE: f32 = 0.02;
    let t = ((deck as f32 - 4.0) / 16.0).clamp(0.0, 1.0);
    LATE + (EARLY - LATE) * t
}

fn quantiles(v: &mut [(i32, f32)]) -> [i32; 3] {
    let total: f32 = v.iter().map(|x| x.1).sum();
    if v.is_empty() || total <= 0.0 {
        return [0; 3];
    }
    v.sort_unstable_by_key(|x| x.0);
    let at = |q: f32| {
        let mut acc = 0.0;
        for &(x, w) in v.iter() {
            acc += w;
            if acc >= q * total {
                return x;
            }
        }
        v[v.len() - 1].0
    };
    [at(0.1), at(0.5), at(0.9)]
}

/// Estimates for seats 1–3 from `deals` deals of the hidden tiles, each also played to the end.
pub fn read_opponents(view: &View, now: Now, deals: usize, seed: u64) -> Vec<Read> {
    read_with(view, Some(now), deals, seed, true)
}

/// With `infer` off every deal counts the same, whatever the players have done. Without `now`
/// the hands are not played on and the chances of opening and finishing stay 0.
pub fn read_with(view: &View, now: Option<Now>, deals: usize, seed: u64, infer: bool) -> Vec<Read> {
    let mut rng = Rng::new(seed);
    let mut sv = Solver::new(&view.rules);
    let (meld_req, pair_req) = view.requirements();
    let hold = holds_ready(view.deck_count);
    let mut pers = [vec![], vec![], vec![]];
    let mut pairs = [vec![], vec![], vec![]];
    let mut points = [vec![], vec![], vec![]];
    let mut either = [0f32; 3];
    let mut okey = [0f32; 3];
    let mut melds = [0f32; 3];
    let mut by_pairs = [0f32; 3];
    let mut ahead = [[0f32; 4]; 3];
    let mut total = 0f32;
    for _ in 0..deals {
        let mut g = view.determinize(&mut rng);
        if infer {
            g.policy = [Policy::FINISHER; 4];
            build_hands(&mut g, view, &mut sv, &mut rng);
        }
        let w = if infer {
            view.plausibility_with(&g, &mut sv, Some(hold))
        } else {
            1.0
        };
        total += w;
        for o in 1..4usize {
            let i = o - 1;
            let p = &g.players[o];
            let v = g.open_search(&mut sv, &p.hand, None).map_or(0, |pl| pl.open_value(&view.rules));
            let gost = p.gosterge_bonus.then_some(view.gosterge);
            let n = pair_stats(&p.hand, &view.rules, gost).0;
            let jokers = p.hand[JOKER as usize] as i32;
            let plain = hand_penalty(&p.hand, &view.rules) - jokers * view.rules.joker_in_hand;
            let pts = plain * if p.opened == Opened::Pairs { 2 } else { 1 };
            let by_melds = v >= meld_req;
            let by_pair = view.rules.pairs_enabled && n >= pair_req as usize;
            melds[i] += if by_melds { w } else { 0.0 };
            by_pairs[i] += if by_pair { w } else { 0.0 };
            either[i] += if by_melds || by_pair { w } else { 0.0 };
            okey[i] += if jokers > 0 { w } else { 0.0 };
            pers[i].push((v, w));
            pairs[i].push((n, w));
            points[i].push((pts, w));
        }
        if let Some(now) = now {
            let seen = play_ahead(&mut g, &mut sv, now);
            for (sum, hit) in ahead.iter_mut().zip(seen) {
                for (x, h) in sum.iter_mut().zip(hit) {
                    *x += if h { w } else { 0.0 };
                }
            }
        }
    }
    let t = total.max(f32::MIN_POSITIVE);
    (0..3)
        .map(|i| {
            let top = pers[i].iter().map(|x| x.0).max().unwrap_or(0).max(meld_req);
            let mut dist = vec![0f32; top as usize + 1];
            pers[i].iter().for_each(|&(v, w)| dist[v as usize] += w / t);
            let mut pd = vec![0f32; MAX_PAIRS + 1];
            pairs[i].iter().for_each(|&(n, w)| pd[n.min(MAX_PAIRS)] += w / t);
            Read {
                seat: i as u8 + 1,
                opened: view.players[i + 1].opened,
                deals: deals as u32,
                per_dist: dist,
                p_melds: melds[i] / t,
                p_pairs: by_pairs[i] / t,
                p_either: either[i] / t,
                p_okey: okey[i] / t,
                p_open_soon: ahead[i][0] / t,
                p_open_end: ahead[i][1] / t,
                p_finish_soon: ahead[i][2] / t,
                p_finish_end: ahead[i][3] / t,
                per: quantiles(&mut pers[i]),
                pairs: pd,
                points: quantiles(&mut points[i]),
                meld_req,
                pair_req,
            }
        })
        .collect()
}
