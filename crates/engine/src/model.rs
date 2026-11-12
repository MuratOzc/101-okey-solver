use crate::agent::Agent;
use crate::features::{Moment, encode};
use crate::game::{Game, OpenChoice, Outcome};
use crate::meld::{Shape, Solver};
use crate::net::Net;
use crate::policy::Policy;
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
    let p = g.current;
    g.policy[p as usize] = Policy::TUNED;
    let best = turns(g, sv, p, taken, net.discards)
        .into_iter()
        .map(|t| (turn_value(net, sv, &t, p), t))
        .min_by(|a, b| a.0.total_cmp(&b.0));
    match best {
        Some((_, t)) => {
            *g = t.after;
            t.end
        }
        None => {
            let end = g.bot_act(sv, p, taken);
            if end.is_none() {
                g.current = (p + 1) % 4;
            }
            end
        }
    }
}

pub fn model_turn(g: &mut Game, sv: &mut Solver, net: &Net, first: bool) -> Option<Outcome> {
    let p = g.current;
    if !first && g.deck.is_empty() {
        return Some(Outcome::DeckOut);
    }
    g.table.new_turn();
    let mut taken = None;
    if !first {
        if draw_or_take(net, g, sv, p).is_some_and(|(draw, take)| take < draw) {
            taken = Some(g.take_top(p));
        } else {
            if g.draw(p).is_none() {
                return Some(Outcome::DeckOut);
            }
        }
    }
    model_act(g, sv, net, taken)
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
