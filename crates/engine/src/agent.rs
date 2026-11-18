//! A common seat-at-the-table interface, so that hand-written bots, the search-based player
//! and a trained model can all be matched against each other.

use crate::analysis::{Analyzer, Decision, Options, Phase, public_view};
use crate::game::{Game, Outcome};
use crate::meld::Solver;
use crate::moves::Move;
use crate::net::Net;
use crate::policy::Policy;
use crate::tile::{Tile, tiles_of};
use std::sync::Arc;

pub trait Agent {
    /// Play one whole turn for `g.current`: draw (unless `first`, the starter's opening turn),
    /// lay whatever it wants, discard. Must leave `g.current` on the next player unless the hand ended.
    /// An agent may only use what that seat can see: its own hand and the public table.
    fn turn(&mut self, g: &mut Game, sv: &mut Solver, first: bool) -> Option<Outcome>;
}

/// A heuristic bot.
pub struct BotAgent {
    pub policy: Policy,
}

impl Agent for BotAgent {
    fn turn(&mut self, g: &mut Game, sv: &mut Solver, first: bool) -> Option<Outcome> {
        g.policy[g.current as usize] = self.policy;
        if first { g.starter_turn(sv) } else { g.bot_turn(sv) }
    }
}

/// Runs `play` with the seats turned so that the player on turn sits at seat 0.
fn at_seat_0(g: &mut Game, play: impl FnOnce(&mut Game) -> Option<Outcome>) -> Option<Outcome> {
    let seat = g.current;
    let mut mine = g.rotated(seat);
    let out = play(&mut mine);
    *g = mine.rotated((4 - seat) % 4);
    out.map(|o| match o {
        Outcome::Finished(mut f) => {
            f.player = (f.player + seat) % 4;
            Outcome::Finished(f)
        }
        other => other,
    })
}

/// The Monte Carlo search player, thinking for a fixed number of simulated deals per decision.
pub struct SearchAgent {
    pub opts: Options,
    pub iterations: usize,
    pub seed: u64,
    pub help: Help,
}

impl Agent for SearchAgent {
    fn turn(&mut self, g: &mut Game, sv: &mut Solver, first: bool) -> Option<Outcome> {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        at_seat_0(g, |mine| search_turn(mine, sv, first, self.iterations, self.seed, &self.opts, &self.help))
    }
}

/// Not a legal player: it sees every hand and the order of the deck, and tries each of its plans
/// against that truth. Its rating is the ceiling the luck of the deal leaves room for.
pub struct OracleAgent {
    pub policy: Policy,
    /// How many of its own later turns are searched the same way inside each trial.
    pub depth: u8,
}

impl Agent for OracleAgent {
    fn turn(&mut self, g: &mut Game, sv: &mut Solver, first: bool) -> Option<Outcome> {
        if !first && g.deck.is_empty() {
            return Some(Outcome::DeckOut);
        }
        at_seat_0(g, |mine| {
            mine.policy[0] = self.policy;
            mine.table.new_turn();
            let mut taken = None;
            if !first {
                if mine.bot_wants_take(sv, 0) {
                    taken = Some(mine.take_top(0));
                } else {
                    mine.draw(0);
                }
            }
            mine.lookahead_act(sv, 0, taken, self.depth)
        })
    }
}

fn decide(
    g: &Game,
    phase: Phase,
    taken: Option<Tile>,
    iterations: usize,
    seed: u64,
    opts: &Options,
    help: &Help,
) -> (Option<Decision>, Option<Move>) {
    let mut view = public_view(g, phase, taken);
    for p in 1..4 {
        if help.peek[p] {
            view.players[p].known = tiles_of(&g.players[p].hand);
        }
    }
    let Ok(mut a) = Analyzer::with_options(view, seed, *opts) else {
        return (None, None);
    };
    if let Some(net) = &help.model {
        a.use_model(net.clone());
    }
    let mut done = 0;
    loop {
        a.step(25);
        done += 25;
        let r = a.report();
        // Screening and look-ahead each need their own share of samples before the answer means anything.
        let deep_ready =
            !opts.lookahead || (r.deep && r.candidates.first().is_some_and(|c| c.n >= iterations / 2)) || done >= iterations * 3;
        if done >= iterations && deep_ready && (!r.screening || done >= iterations * 8) {
            return r.candidates.first().map_or((None, None), |c| (Some(c.decision), c.mv.clone()));
        }
    }
}

#[derive(Clone, Default)]
pub struct Help {
    /// The seats whose hands it is told, though never the order of the deck. Not a legal player.
    pub peek: [bool; 4],
    /// The network that plays the searcher's later turns inside its simulations.
    pub model: Option<Arc<Net>>,
}

/// One whole turn for seat 0 of `g`, chosen by search. Falls back to the bot if the search has no answer.
pub fn search_turn(
    g: &mut Game,
    sv: &mut Solver,
    first: bool,
    iterations: usize,
    seed: u64,
    opts: &Options,
    help: &Help,
) -> Option<Outcome> {
    g.policy[0] = opts.policy;
    if !first && g.deck.is_empty() {
        return Some(Outcome::DeckOut);
    }
    g.table.new_turn();
    let mut taken = None;
    if !first {
        match decide(g, Phase::BeforeDraw, None, iterations, seed, opts, help).0 {
            Some(Decision::Take) => taken = Some(g.take_top(0)),
            _ => {
                g.draw(0);
            }
        }
    }
    let (picked, mv) = decide(g, Phase::AfterDraw, taken, iterations, seed ^ 0x55, opts, help);
    let backup = g.clone();
    let res = match (picked, mv) {
        (Some(Decision::Play(_)), Some(mv)) => g.play_move(0, &mv, taken),
        (Some(Decision::Act { choice, discard }), _) => g.apply_choice(sv, 0, choice, taken).inspect(|r| {
            if r.is_none() {
                g.discard_or_bot(sv, 0, discard);
            }
        }),
        _ => Err(()),
    };
    let res = match res {
        Ok(r) => r,
        Err(()) => {
            *g = backup;
            g.bot_act(sv, 0, taken)
        }
    };
    if res.is_none() {
        g.current = 1;
    }
    res
}

/// Play one dealt hand to the end with four agents, `seats[i]` sitting at seat i. Returns the scores by seat.
pub fn play_hand(g: &mut Game, sv: &mut Solver, seats: &mut [&mut dyn Agent; 4]) -> [i32; 4] {
    let mut first = true;
    let out = loop {
        let p = g.current as usize;
        if let Some(o) = seats[p].turn(g, sv, first) {
            break o;
        }
        first = false;
    };
    g.scores(&out)
}
