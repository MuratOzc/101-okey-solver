//! A common seat-at-the-table interface, so that hand-written bots, the search-based player
//! and (later) a trained model can all be matched against each other and rated.

use crate::analysis::{Analyzer, Decision, Options, Phase, public_view};
use crate::game::{Game, Outcome};
use crate::meld::Solver;
use crate::moves::Move;
use crate::policy::Policy;
use crate::stakes::erf;
use crate::tile::Tile;

pub trait Agent {
    fn name(&self) -> String;

    /// Play one whole turn for `g.current`: draw (unless `first`, the starter's opening turn),
    /// lay whatever it wants, discard. Must leave `g.current` on the next player unless the hand ended.
    /// An agent may only use what that seat can see: its own hand and the public table.
    fn turn(&mut self, g: &mut Game, sv: &mut Solver, first: bool) -> Option<Outcome>;
}

/// A heuristic bot.
pub struct BotAgent {
    pub label: String,
    pub policy: Policy,
}

impl Agent for BotAgent {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn turn(&mut self, g: &mut Game, sv: &mut Solver, first: bool) -> Option<Outcome> {
        g.policy[g.current as usize] = self.policy;
        if first { g.starter_turn(sv) } else { g.bot_turn(sv) }
    }
}

/// The Monte Carlo search player, thinking for a fixed number of simulated deals per decision.
pub struct SearchAgent {
    pub label: String,
    pub opts: Options,
    pub iterations: usize,
    pub seed: u64,
}

impl Agent for SearchAgent {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn turn(&mut self, g: &mut Game, sv: &mut Solver, first: bool) -> Option<Outcome> {
        let seat = g.current;
        let mut mine = g.rotated(seat);
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let out = search_turn(&mut mine, sv, first, self.iterations, self.seed, &self.opts);
        *g = mine.rotated((4 - seat) % 4);
        out.map(|o| match o {
            Outcome::Finished(mut f) => {
                f.player = (f.player + seat) % 4;
                Outcome::Finished(f)
            }
            other => other,
        })
    }
}

/// Not a legal player: it sees every hand and the order of the deck, and tries each of its plans
/// against that truth. Its rating is the ceiling the luck of the deal leaves room for.
pub struct OracleAgent {
    pub label: String,
    pub policy: Policy,
    /// How many of its own later turns are searched the same way inside each trial.
    pub depth: u8,
}

impl Agent for OracleAgent {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn turn(&mut self, g: &mut Game, sv: &mut Solver, first: bool) -> Option<Outcome> {
        let seat = g.current;
        let mut mine = g.rotated(seat);
        mine.policy[0] = self.policy;
        mine.table.new_turn();
        let mut taken = None;
        let mut out = None;
        if !first {
            if mine.bot_wants_take(sv, 0) {
                taken = Some(mine.take_top(0));
            } else {
                match mine.deck.pop() {
                    Some(t) => mine.players[0].hand[t as usize] += 1,
                    None => out = Some(Outcome::DeckOut),
                }
            }
        }
        if out.is_none() {
            out = mine.lookahead_act(sv, 0, taken, self.depth);
        }
        *g = mine.rotated((4 - seat) % 4);
        out.map(|o| match o {
            Outcome::Finished(mut f) => {
                f.player = (f.player + seat) % 4;
                Outcome::Finished(f)
            }
            other => other,
        })
    }
}

fn decide(g: &Game, phase: Phase, taken: Option<Tile>, iterations: usize, seed: u64, opts: &Options) -> (Option<Decision>, Option<Move>) {
    let Ok(mut a) = Analyzer::with_options(public_view(g, phase, taken), seed, *opts) else {
        return (None, None);
    };
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

/// One whole turn for seat 0 of `g`, chosen by search. Falls back to the bot if the search has no answer.
pub fn search_turn(g: &mut Game, sv: &mut Solver, first: bool, iterations: usize, seed: u64, opts: &Options) -> Option<Outcome> {
    g.policy[0] = opts.policy;
    g.table.new_turn();
    let mut taken = None;
    if !first {
        match decide(g, Phase::BeforeDraw, None, iterations, seed, opts).0 {
            Some(Decision::Take) => taken = Some(g.take_top(0)),
            _ => match g.deck.pop() {
                Some(t) => g.players[0].hand[t as usize] += 1,
                None => return Some(Outcome::DeckOut),
            },
        }
    }
    let (picked, mv) = decide(g, Phase::AfterDraw, taken, iterations, seed ^ 0x55, opts);
    let backup = g.clone();
    let res = match (picked, mv) {
        (Some(Decision::Play(_)), Some(mv)) => g.play_move(0, &mv, taken),
        (Some(Decision::Act { choice, discard }), _) => g.apply_choice(sv, 0, choice, taken).inspect(|r| {
            if r.is_none() {
                let t = discard
                    .filter(|&t| g.players[0].hand[t as usize] > 0)
                    .unwrap_or_else(|| g.bot_discard(sv, 0));
                g.discard(0, t);
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

/// Ratings on the Elo scale from pairwise results, by the Bradley–Terry model:
/// a 400-point lead means winning a head-to-head comparison ten times in eleven.
/// `wins[i][j]` counts the hands in which i scored better than j (a tie is half each).
/// The first player is pinned at `anchor`.
pub fn elo_from_pairs(wins: &[Vec<f64>], anchor: f64) -> Vec<f64> {
    let n = wins.len();
    let mut strength = vec![1.0f64; n];
    for _ in 0..2000 {
        let mut next = strength.clone();
        for i in 0..n {
            let won: f64 = (0..n).map(|j| wins[i][j]).sum();
            let denom: f64 = (0..n)
                .filter(|&j| j != i)
                .map(|j| (wins[i][j] + wins[j][i]) / (strength[i] + strength[j]))
                .sum();
            if denom > 0.0 {
                // A player with no wins at all would sink to minus infinity; half a win keeps it finite.
                next[i] = won.max(0.5) / denom;
            }
        }
        let scale = next[0];
        next.iter_mut().for_each(|s| *s /= scale);
        strength = next;
    }
    strength.iter().map(|s| anchor + 400.0 * s.log10()).collect()
}

/// The chance that A out-scores B over a whole game of `hands` hands, from per-hand score
/// differences (A minus B; lower is better) seen when both sat at the same table.
/// Hands are independent deals, so the game total is close to normal.
pub fn game_win_chance(diffs: &[f32], hands: u32) -> f64 {
    let n = diffs.len() as f64;
    if n < 2.0 {
        return 0.5;
    }
    let mean = diffs.iter().map(|&d| d as f64).sum::<f64>() / n;
    let var = diffs.iter().map(|&d| (d as f64 - mean).powi(2)).sum::<f64>() / (n - 1.0);
    if var <= 0.0 {
        return if mean < 0.0 {
            1.0
        } else if mean > 0.0 {
            0.0
        } else {
            0.5
        };
    }
    let z = -mean * (hands as f64).sqrt() / var.sqrt();
    0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2))
}
