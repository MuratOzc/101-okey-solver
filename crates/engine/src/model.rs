use crate::agent::Agent;
use crate::features::{Feat, Moment, encode};
use crate::game::{Game, OpenChoice, Opened, Outcome};
use crate::meld::{Shape, Solver};
use crate::moves::generate_some;
use crate::net::{Near, Net};
use crate::policy::Policy;
use crate::rng::Rng;
use crate::tile::*;
use std::sync::Arc;

pub struct Turn {
    pub choice: OpenChoice,
    pub discard: Option<Tile>,
    pub after: Game,
    pub end: Option<Outcome>,
}

// The network predicts the gap without penalties already written down; training labels subtract the same.
pub fn written3(g: &Game, me: u8) -> i32 {
    g.gap3(&[0, 1, 2, 3].map(|q| g.players[q].penalty), me)
}

pub fn turns(g: &Game, sv: &mut Solver, p: u8, taken: Option<Tile>, discards: usize) -> Vec<Turn> {
    let pi = p as usize;
    let mut out = Vec::new();
    let mut seen: Vec<(Counts, Vec<(u8, Shape)>, i32, bool)> = Vec::new();
    for choice in Game::plan_choices(g.players[pi].opened, true) {
        let mut a = g.clone();
        let Ok(end) = a.apply_choice(sv, p, choice, taken) else { continue };
        let key = (
            a.players[pi].hand,
            a.table.melds.iter().map(|m| (m.owner, m.shape)).collect::<Vec<_>>(),
            a.players.iter().map(|pl| pl.penalty).sum::<i32>(),
            end.is_some(),
        );
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if end.is_some() {
            out.push(Turn {
                choice,
                discard: None,
                after: a,
                end,
            });
            continue;
        }
        if a.hand_len(p) < 2 {
            continue;
        }
        let mut ranked = a.discard_scores(sv, p);
        ranked.sort_by_key(|&(_, keep)| keep);
        for (t, _) in ranked.into_iter().take(discards) {
            let mut b = a.clone();
            b.discard(p, t);
            b.current = (p + 1) % 4;
            out.push(Turn {
                choice,
                discard: Some(t),
                after: b,
                end: None,
            });
        }
    }
    out
}

pub fn turn_value(net: &Net, sv: &mut Solver, t: &Turn, me: u8) -> f32 {
    match &t.end {
        Some(end) => t.after.gap3(&t.after.scores(end), me) as f32,
        None => written3(&t.after, me) as f32 + net.value(&encode(&t.after, sv, me, Moment::After, net.full())),
    }
}

pub fn draw_or_take(net: &Net, g: &Game, sv: &mut Solver, p: u8) -> Option<(f32, f32)> {
    let t = g.top.filter(|&t| g.can_use_taken(sv, p, t))?;
    let draw = written3(g, p) as f32 + net.value(&encode(g, sv, p, Moment::Draw, net.full()));
    let mut h = g.clone();
    h.take_top(p);
    let take = written3(&h, p) as f32 + net.value(&encode(&h, sv, p, Moment::Take(t), net.full()));
    Some((draw, take))
}

pub fn model_act(g: &mut Game, sv: &mut Solver, net: &Net, taken: Option<Tile>) -> Option<Outcome> {
    model_act_valued(g, sv, net, taken).0
}

/// Also returns what the network expects of the position it moved to, when the hand goes on from there.
pub fn model_act_valued(g: &mut Game, sv: &mut Solver, net: &Net, taken: Option<Tile>) -> (Option<Outcome>, Option<f32>) {
    let p = g.current;
    g.policy[p as usize] = Policy::TUNED;
    let best = turns(g, sv, p, taken, net.discards)
        .into_iter()
        .map(|t| (turn_value(net, sv, &t, p), t))
        .min_by(|a, b| a.0.total_cmp(&b.0));
    match best {
        Some((v, t)) => {
            *g = t.after;
            let v = t.end.is_none().then_some(v);
            (t.end, v)
        }
        None => {
            let end = g.bot_act(sv, p, taken);
            if end.is_none() {
                g.current = (p + 1) % 4;
            }
            (end, None)
        }
    }
}

/// Above this many moves in one turn, only the most promising positions have every discard compared.
const SCREEN_ABOVE: usize = 60;
const SCREEN_KEEP: usize = 6;
const ALL_POSITIONS: usize = 3000;
/// A hand that can open usually can in hundreds of nearly equal ways; this many of them are compared.
const OPENINGS_TRIED: usize = 150;
/// A position differing from the last fully valued one in more inputs than this is valued from scratch.
const NEAR_ENOUGH: u32 = 48;

struct Valuer<'a> {
    net: &'a Net,
    near: Option<Near>,
}

impl Valuer<'_> {
    fn value(&mut self, f: &Feat) -> f32 {
        match &self.near {
            Some(near) if Net::distance(near, f) <= NEAR_ENOUGH => self.net.value_near(near, f),
            _ => {
                let near = self.net.near(f);
                let v = self.net.value_near(&near, f);
                self.near = Some(near);
                v
            }
        }
    }
}

/// What the network makes of `a` once `p` has discarded `t` there. Leaves `a` as it was.
fn after_discard(a: &mut Game, sv: &mut Solver, valuer: &mut Valuer, p: u8, t: Tile) -> f32 {
    let pi = p as usize;
    let (penalty, last, top) = (a.players[pi].penalty, a.last[pi], a.top);
    a.discard(p, t);
    let v = written3(a, p) as f32 + valuer.value(&encode(a, sv, p, Moment::After, valuer.net.full()));
    a.players[pi].penalty = penalty;
    a.players[pi].hand[t as usize] += 1;
    a.seen[t as usize] -= 1;
    a.piles[pi][t as usize] -= 1;
    a.last[pi] = last;
    a.top = top;
    v
}

/// The network's turn among every legal move, not only the ready-made plans. With very many moves the
/// positions are first compared on one discard each, the one that looked best where every discard was tried.
pub fn model_act_all(g: &mut Game, sv: &mut Solver, net: &Net, taken: Option<Tile>) -> (Option<Outcome>, Option<f32>) {
    let p = g.current;
    let pi = p as usize;
    let was_unopened = g.players[pi].opened == Opened::No;
    let mut valuer = Valuer { net, near: None };
    let mut open: Vec<(Game, Vec<Tile>)> = Vec::new();
    let mut best: Option<(f32, usize, Tile)> = None;
    let mut finish: Option<(f32, Game, Outcome)> = None;
    let seed = g.players[pi]
        .hand
        .iter()
        .fold(g.deck.len() as u64, |h, &n| h.wrapping_mul(31).wrapping_add(n as u64));
    for acts in generate_some(g, p, taken, ALL_POSITIONS, OPENINGS_TRIED, &mut Rng::new(seed)).positions {
        let mut a = g.clone();
        let Ok(done) = a.play_acts(p, &acts, taken) else { continue };
        let hand = a.players[pi].hand;
        let finishing = a.hand_len(p) == 1;
        if done.short_opening {
            if !finishing || !done.only_own_melds || a.rules.elden_requires_open_min {
                continue;
            }
            a.players[pi].opened = if done.pairs { Opened::Pairs } else { Opened::Melds };
        }
        if finishing {
            let Some(t) = (0..NIDS as Tile).find(|&t| hand[t as usize] > 0) else {
                continue;
            };
            let elden = was_unopened && (done.only_own_melds || a.rules.open_finish_is_elden);
            let end = a.finish(p, t, elden);
            let v = a.gap3(&a.scores(&end), p) as f32;
            if finish.as_ref().is_none_or(|f| v < f.0) {
                finish = Some((v, a, end));
            }
            continue;
        }
        let mut ds: Vec<Tile> = (0..NIDS as Tile).filter(|&t| hand[t as usize] > 0).collect();
        let cheapest = ds.iter().map(|&t| a.discard_penalty(p, t)).min().unwrap_or(0);
        ds.retain(|&t| a.discard_penalty(p, t) == cheapest);
        open.push((a, ds));
    }
    let mut every = |k: usize, open: &mut Vec<(Game, Vec<Tile>)>, liked: &mut [f32; NIDS], sv: &mut Solver, valuer: &mut Valuer| {
        let (a, ds) = &mut open[k];
        for &t in ds.iter() {
            let v = after_discard(a, sv, valuer, p, t);
            liked[t as usize] = liked[t as usize].min(v);
            if best.is_none_or(|b| v < b.0) {
                best = Some((v, k, t));
            }
        }
    };
    let mut liked = [f32::INFINITY; NIDS];
    if open.iter().map(|(_, ds)| ds.len()).sum::<usize>() <= SCREEN_ABOVE {
        for k in 0..open.len() {
            every(k, &mut open, &mut liked, sv, &mut valuer);
        }
    } else {
        every(0, &mut open, &mut liked, sv, &mut valuer);
        let mut first: Vec<(f32, usize)> = Vec::with_capacity(open.len());
        for (k, (a, ds)) in open.iter_mut().enumerate().skip(1) {
            let Some(&t) = ds.iter().min_by(|&&x, &&y| liked[x as usize].total_cmp(&liked[y as usize])) else {
                continue;
            };
            first.push((after_discard(a, sv, &mut valuer, p, t), k));
        }
        first.sort_by(|x, y| x.0.total_cmp(&y.0));
        for &(_, k) in first.iter().take(SCREEN_KEEP) {
            every(k, &mut open, &mut liked, sv, &mut valuer);
        }
    }
    match (finish, best) {
        (Some((fv, a, end)), b) if b.is_none_or(|b| fv <= b.0) => {
            *g = a;
            (Some(end), None)
        }
        (_, Some((v, k, t))) => {
            let (mut a, _) = open.swap_remove(k);
            a.discard(p, t);
            a.current = (p + 1) % 4;
            *g = a;
            (None, Some(v))
        }
        (None, None) => {
            let end = g.bot_act(sv, p, taken);
            if end.is_none() {
                g.current = (p + 1) % 4;
            }
            (end, None)
        }
        (Some(_), None) => unreachable!(),
    }
}

/// A whole turn of the network among every legal move: draw or take, then `model_act_all`.
pub fn model_turn_all(g: &mut Game, sv: &mut Solver, net: &Net, first: bool) -> Option<Outcome> {
    let p = g.current;
    if !first && g.deck.is_empty() {
        return Some(Outcome::DeckOut);
    }
    g.table.new_turn();
    let mut taken = None;
    if !first {
        if draw_or_take(net, g, sv, p).is_some_and(|(draw, take)| take < draw) {
            taken = Some(g.take_top(p));
        } else if g.draw(p).is_none() {
            return Some(Outcome::DeckOut);
        }
    }
    model_act_all(g, sv, net, taken).0
}

pub fn model_turn(g: &mut Game, sv: &mut Solver, net: &Net, first: bool) -> Option<Outcome> {
    model_turn_valued(g, sv, net, first).0
}

pub fn model_turn_valued(g: &mut Game, sv: &mut Solver, net: &Net, first: bool) -> (Option<Outcome>, Option<f32>) {
    let p = g.current;
    if !first && g.deck.is_empty() {
        return (Some(Outcome::DeckOut), None);
    }
    g.table.new_turn();
    let mut taken = None;
    if !first {
        if draw_or_take(net, g, sv, p).is_some_and(|(draw, take)| take < draw) {
            taken = Some(g.take_top(p));
        } else if g.draw(p).is_none() {
            return (Some(Outcome::DeckOut), None);
        }
    }
    model_act_valued(g, sv, net, taken)
}

/// Plays on until seat `me` has moved `horizon` more times and returns, as the error, what the network
/// expects of the position it then stands in. A hand that ends sooner, or a horizon of 0, gives the outcome.
pub fn play_out_to(g: &mut Game, sv: &mut Solver, net: &Net, model: [bool; 4], me: u8, horizon: u8) -> Result<Outcome, f32> {
    let mut left = horizon;
    for _ in 0..400 {
        let p = g.current;
        let end = if p == me && left > 0 {
            let (end, v) = model_turn_valued(g, sv, net, false);
            left -= 1;
            if let (None, Some(v), 0) = (&end, v, left) {
                return Err(v);
            }
            end
        } else if model[p as usize] {
            model_turn(g, sv, net, false)
        } else {
            g.bot_turn(sv)
        };
        if let Some(end) = end {
            return Ok(end);
        }
    }
    Ok(Outcome::DeckOut)
}

pub fn play_out_with(g: &mut Game, sv: &mut Solver, net: &Net, model: [bool; 4]) -> Outcome {
    for _ in 0..400 {
        let end = if model[g.current as usize] {
            model_turn(g, sv, net, false)
        } else {
            g.bot_turn(sv)
        };
        if let Some(end) = end {
            return end;
        }
    }
    Outcome::DeckOut
}

pub struct ModelAgent {
    pub label: String,
    pub net: Arc<Net>,
}

impl Agent for ModelAgent {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn turn(&mut self, g: &mut Game, sv: &mut Solver, first: bool) -> Option<Outcome> {
        model_turn(g, sv, &self.net, first)
    }
}
