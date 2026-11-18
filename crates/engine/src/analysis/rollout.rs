//! One simulated hand after a candidate, and what it is scored by.

use super::*;

pub(super) fn pack(r: &RolloutResult) -> Packed {
    let flags = r.finish as i32
        | (r.opened as i32) << 1
        | (r.next_takes as i32) << 2
        | (r.whole as i32) << 3
        | r.last_draw.map_or(0, |p| p as i32 + 1) << 4;
    [r.gap, r.tie, (r.win as f64 * 1e6).round() as i32, flags]
}

/// How the network plays my later turns inside one simulation.
#[derive(Clone, Copy)]
pub(super) struct Ai<'a> {
    pub(super) net: &'a Net,
    pub(super) horizon: u8,
    /// Play on past the horizon to the end of the hand, for what only a finished hand can tell.
    pub(super) whole: bool,
}

/// Gaps are stored times this so the three-opponent average stays an integer.
pub(super) const GAP_SCALE: f64 = 3.0;
/// The chance of losing the match is stored times this, fine enough for a match that is nearly lost.
pub(super) const MATCH_SCALE: f64 = 3_000_000.0;

pub(super) struct RolloutResult {
    /// GAP_SCALE × (my score − opponents' average); team against team when partnered.
    /// With a known match situation: GAP_SCALE × the chance of losing the match, per mille.
    pub(super) gap: i32,
    /// Chance of winning the match in this outcome (0 when no match situation is known).
    pub(super) win: f32,
    /// The plain score gap, for ordering outcomes that are level on `gap`.
    pub(super) tie: i32,
    pub(super) finish: bool,
    pub(super) opened: bool,
    pub(super) next_takes: bool,
    /// Who drew the deck's last tile, counted from me.
    pub(super) last_draw: Option<u8>,
    /// The hand was played to its end, so everything but `gap` and `tie` describes a real ending.
    pub(super) whole: bool,
}

pub(super) fn play_decision_on(view: &View, mut g: Game, sv: &mut Solver, d: Decision, depth: u8, ai: Option<Ai>) -> RolloutResult {
    let mut out = None;
    let mut rest = depth;
    match d {
        Decision::Draw | Decision::Take if g.deck.is_empty() => out = Some(Outcome::DeckOut),
        Decision::Draw | Decision::Take => {
            let taken = (d == Decision::Take).then(|| g.take_top(0));
            if taken.is_none() {
                g.draw(0);
            }
            out = match ai {
                Some(ai) => model_act(&mut g, sv, ai.net, taken),
                None if depth > 0 => g.lookahead_act(sv, 0, taken, depth - 1),
                None => g.bot_act(sv, 0, taken),
            };
            rest = depth.saturating_sub(1);
        }
        Decision::Play(_) => g.discard_or_bot(sv, 0, None),
        Decision::Act { choice, discard } => match g.apply_choice(sv, 0, choice, view.taken) {
            Ok(Some(o)) => out = Some(o),
            Ok(None) => g.discard_or_bot(sv, 0, discard),
            Err(()) => g.discard_or_bot(sv, 0, None),
        },
    }
    conclude(view, g, out, sv, rest, ai)
}

pub(super) fn play_move_on(view: &View, mut g: Game, sv: &mut Solver, mv: &Move, depth: u8, ai: Option<Ai>) -> RolloutResult {
    let out = match g.play_move(0, mv, view.taken) {
        Ok(o) => o,
        Err(()) => {
            g.discard_or_bot(sv, 0, None);
            None
        }
    };
    conclude(view, g, out, sv, depth, ai)
}

/// Play the hand out from just after seat 0's turn, and score it.
pub(super) fn conclude(view: &View, mut g: Game, out: Option<Outcome>, sv: &mut Solver, depth: u8, ai: Option<Ai>) -> RolloutResult {
    let mut next_takes = false;
    let mut expected = None;
    let o = match out {
        Some(o) => o,
        None => {
            g.current = 1;
            next_takes = !g.deck.is_empty() && g.bot_wants_take(sv, 1);
            if let Some(ai) = ai {
                match play_out_to(&mut g, sv, ai.net, MINE, 0, ai.horizon) {
                    Ok(o) => o,
                    Err(value) if ai.whole => {
                        expected = Some(value.round() as i32);
                        play_out_with(&mut g, sv, ai.net, MINE)
                    }
                    Err(value) => {
                        let gap = value.round() as i32;
                        return RolloutResult {
                            gap,
                            win: 0.0,
                            tie: gap,
                            finish: false,
                            opened: false,
                            next_takes,
                            last_draw: None,
                            whole: false,
                        };
                    }
                }
            } else if depth > 0 {
                g.play_out_lookahead(sv, 0, depth)
            } else {
                g.play_out(sv)
            }
        }
    };
    let s = g.scores(&o);
    let tie = expected.unwrap_or_else(|| g.gap3(&s, 0));
    let loss = g.stakes.map(|st| st.loss(&s, view.rules.partners));
    let gap = loss.map_or(tie, |l| (l * MATCH_SCALE).round() as i32);
    let win = loss.map_or(0.0, |l| 1.0 - l as f32);
    let finish = matches!(o, Outcome::Finished(f) if f.player == 0);
    RolloutResult {
        gap,
        win,
        tie,
        finish,
        opened: finish || g.players[0].opened != Opened::No,
        next_takes,
        last_draw: g.last_draw,
        // Hands that happen to end inside the horizon must not be counted more often than the rest.
        whole: ai.is_none_or(|a| a.whole || a.horizon == 0),
    }
}
