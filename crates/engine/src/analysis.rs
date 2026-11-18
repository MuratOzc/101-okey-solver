use crate::game::*;
use crate::meld::{Shape, Solver};
use crate::model::{model_act, play_out_to, play_out_with};
use crate::moves::{Act, Move, generate};
use crate::net::Net;
use crate::pairs::pair_plan;
use crate::policy::{LayStyle, OpenStyle, Policy};
use crate::rng::Rng;
use crate::rules::Rules;
use crate::stakes::Stakes;
use crate::table::{Table, TableMeld};
use crate::tile::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

mod describe;
mod infer;
mod rollout;
mod view;
pub use describe::*;
use rollout::*;
pub use view::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Decision {
    Draw,
    Take,
    Act {
        choice: OpenChoice,
        discard: Option<Tile>,
    },
    /// An explicit move from the exhaustive generator: position index × 64 + discarded tile.
    Play(u32),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CandReport {
    pub decision: Decision,
    pub label: String,
    /// Expected score gap: mine minus the opponents' average (team minus team when partnered). Lower is better.
    pub mean: f64,
    pub diff_se: f64,
    pub n: usize,
    pub p_finish: f64,
    pub p_open: f64,
    /// How often the next player takes the tile this move discards.
    pub p_next_takes: f64,
    /// For each seat counted from me: how often it draws the deck's last tile.
    #[serde(default)]
    pub p_last_draw: [f64; 4],
    /// How many of the deals were played to the end of the hand; the chances above come from those.
    #[serde(default)]
    pub whole: usize,
    /// Chance of winning the match, when the match situation is known. `mean` is then the chance
    /// of losing it, per mille, instead of a score gap.
    #[serde(default)]
    pub p_win: Option<f64>,
    /// Expected score gap of this hand, whatever is being played for.
    #[serde(default)]
    pub gap: f64,
    pub alive: bool,
    /// The explicit steps, for the leading candidates of an exhaustive search.
    #[serde(default)]
    pub mv: Option<Move>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Report {
    pub iterations: u64,
    pub candidates: Vec<CandReport>,
    /// How many distinct positions the exhaustive generator found (0 when it was not used).
    pub positions: usize,
    /// Still narrowing all positions down; discards are not compared yet.
    pub screening: bool,
    /// The generator hit its cap, so some legal moves were never looked at.
    pub truncated: bool,
    /// The remaining candidates are being compared with look-ahead simulations.
    pub deep: bool,
    pub model: bool,
    /// No other live candidate can still be better than the leader by more than a point or two.
    pub settled: bool,
    /// How much better than the leader another live candidate could still be, in the units of `mean`.
    pub doubt: f64,
    /// The runner-up is within the noise of the leader.
    pub close: bool,
}

/// The live candidates, in the order their results must come back.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub version: u32,
    pub depth: u8,
    pub cands: Vec<Decision>,
}

/// One candidate's result on one deal: gap, tie, chance of winning per million, flags.
pub type Packed = [i32; 4];

struct Cand {
    decision: Decision,
    mv: Option<Move>,
    scores: Vec<i32>,
    /// Summed score gap, used only to order candidates whose `scores` are level.
    tie: i64,
    win: f64,
    finish: u32,
    open: u32,
    next_takes: u32,
    last_draw: [u32; 4],
    /// Deals played to the end of the hand; the counts above come from these alone.
    whole: u32,
    alive: bool,
}

pub struct Analyzer {
    opts: Options,
    view: View,
    sv: Solver,
    cands: Vec<Cand>,
    iter: u64,
    seed: u64,
    positions: Vec<Vec<Act>>,
    discards: Vec<Vec<Tile>>,
    screening: bool,
    next_cut: usize,
    truncated: bool,
    /// How many of my later turns each simulation searches (0 = the bot plays them).
    depth: u8,
    net: Option<Arc<Net>>,
    version: u32,
    sent: std::collections::VecDeque<Field>,
    work: Vec<(Decision, Option<Move>)>,
    work_version: Option<u32>,
    work_depth: u8,
    next_prune: u64,
}

/// Look-ahead costs roughly ten plain simulations each, so it starts only when this few candidates are left.
const DEEP_FIELD: usize = 6;
const DEEP_AFTER: usize = 24;

const POSITION_CAP: usize = 20_000;
/// Screening halves the field until this many positions remain; then every discard of each is compared.
const SCREEN_KEEP: usize = 12;
/// With no more moves than this, skip screening: every move competes under the careful pruning rule.
const SCREEN_ABOVE_MOVES: usize = 300;
const FIRST_CUT: usize = 16;
const FIELDS_KEPT: usize = 48;
// Fewer deals than this hide the rare big swings, and a steadily cheaper-looking move gets cut for good.
const PRUNE_AFTER: usize = 48;
// Measured with examples/settle.rs: at 2 errors, 300 deals and 1.5 points, stopping cost 0.27 points a decision
// against a 12000-deal run; with these it costs nothing measurable.
const SETTLE_AFTER: usize = 600;
const SETTLE_SURE: f64 = 3.0;
/// Points per hand (or per mille of match chance) that are not worth more waiting.
const SETTLE_WITHIN: f64 = 1.0;
const SCREEN_DOUBT: f64 = 2.0;
const MINE: [bool; 4] = [true, false, false, false];
/// With a horizon, one deal in this many is still played to the end.
const WHOLE_EVERY: usize = 4;
const REPORT_ROWS: usize = 40;
const MOVES_SHOWN: usize = 12;

fn blank(decision: Decision, mv: Option<Move>) -> Cand {
    Cand {
        decision,
        mv,
        scores: vec![],
        tie: 0,
        win: 0.0,
        finish: 0,
        open: 0,
        next_takes: 0,
        last_draw: [0; 4],
        whole: 0,
        alive: true,
    }
}

/// How the analyzer searches and what it assumes about the other players.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// How every simulated player (me in later turns, and the opponents) is assumed to play.
    pub policy: Policy,
    pub max_discards: usize,
    /// Deal the hidden tiles so that they fit what the opponents have done so far,
    /// instead of uniformly at random.
    pub infer: bool,
    /// Once few candidates remain, simulate my own later turns by search instead of by the bot.
    /// Off by default: in the arena it cost ten times the work and changed nothing (±8 points/hand).
    pub lookahead: bool,
    /// With the network playing my later turns: stop each simulation after this many of them and take
    /// what the network expects from there. 0 plays every simulation to the end of the hand.
    pub horizon: u8,
    /// Give unopened opponents hands built over their turns so far instead of loose random ones.
    pub build: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            policy: Policy::HOLDER,
            max_discards: 12,
            infer: true,
            lookahead: false,
            horizon: 1,
            build: false,
        }
    }
}

// A network knows only the plain score gap under the default rules, alone or in teams as it was trained.
pub fn model_fits(view: &View, partners: bool) -> bool {
    let trained = Rules {
        partners,
        ..Rules::default()
    };
    !decisive(view) && view.rules == trained
}

// In the last hands finish-or-wait is the whole question, and the network, trained for points, takes any finish.
fn decisive(view: &View) -> bool {
    view.stakes.is_some_and(|s| s.hands_left <= 1)
}

impl Analyzer {
    pub fn new(view: View, seed: u64) -> Result<Self, String> {
        Self::with_options(view, seed, Options::default())
    }

    pub fn with_options(view: View, seed: u64, opts: Options) -> Result<Self, String> {
        let probs = view.problems();
        if !probs.is_empty() {
            return Err(probs.join("; "));
        }
        let mut sv = Solver::new(&view.rules);
        let mut me = Analyzer {
            opts,
            view,
            sv: Solver::new(&Rules::default()),
            cands: vec![],
            iter: 0,
            seed,
            positions: vec![],
            discards: vec![],
            screening: false,
            next_cut: FIRST_CUT,
            truncated: false,
            depth: 0,
            net: None,
            version: 0,
            sent: Default::default(),
            work: vec![],
            work_version: None,
            work_depth: 0,
            next_prune: 0,
        };
        // Playing for the match, what decides is finish-or-wait and which tile to give up, including
        // tiles that cost a penalty; a winning line may come up on one deal in fifty, so the field has to
        // stay small enough to replay every candidate on every deal. Plans with every discard do that.
        let plans = Options {
            max_discards: NIDS,
            ..opts
        };
        if decisive(&me.view) && me.view.phase == Phase::AfterDraw {
            me.cands = candidates(&me.view, &mut sv, &plans).into_iter().map(|d| blank(d, None)).collect();
        } else if me.view.phase == Phase::AfterDraw {
            me.enumerate(&mut sv);
            // A hand with more arrangements than the generator's cap cannot be screened in any useful time.
            if me.truncated {
                me.positions.clear();
                me.discards.clear();
                me.screening = false;
                me.cands = candidates(&me.view, &mut sv, &opts).into_iter().map(|d| blank(d, None)).collect();
            }
        } else {
            me.cands = candidates(&me.view, &mut sv, &opts).into_iter().map(|d| blank(d, None)).collect();
        }
        me.sv = sv;
        if me.cands.is_empty() {
            return Err("Geçerli hamle bulunamadı".into());
        }
        Ok(me)
    }

    /// Let the network play my later turns inside every simulation, where it was trained for the rules.
    pub fn use_model(&mut self, net: Arc<Net>) {
        if model_fits(&self.view, net.partners) {
            self.net = Some(net);
        }
    }

    /// List every legal position; start by screening them with one sensible discard each.
    fn enumerate(&mut self, sv: &mut Solver) {
        let mut g = self.view.determinize(&mut Rng::new(1));
        g.policy = [self.opts.policy; 4];
        let found = generate(&g, 0, self.view.taken, POSITION_CAP);
        self.truncated = found.truncated;
        let mut defaults = Vec::new();
        for acts in found.positions {
            let mut g2 = g.clone();
            if g2.play_acts(0, &acts, self.view.taken).is_err() {
                continue;
            }
            let hand = g2.players[0].hand;
            let mut ds: Vec<Tile> = (0..NIDS as Tile).filter(|&t| hand[t as usize] > 0).collect();
            if ds.is_empty() {
                continue;
            }
            // Throwing an playable tile or an okey costs a flat penalty: never worth it while a free discard exists.
            let cheapest = ds.iter().map(|&t| g2.discard_penalty(0, t)).min().unwrap_or(0);
            ds.retain(|&t| g2.discard_penalty(0, t) == cheapest);
            defaults.push(if total(&hand) == 1 { ds[0] } else { g2.bot_discard(sv, 0) });
            self.positions.push(acts);
            self.discards.push(ds);
        }
        let moves: usize = self.discards.iter().map(Vec::len).sum();
        if moves <= SCREEN_ABOVE_MOVES || self.positions.len() <= SCREEN_KEEP {
            let all: Vec<usize> = (0..self.positions.len()).collect();
            self.expand(&all);
        } else {
            self.screening = true;
            self.cands = (0..self.positions.len()).map(|i| self.candidate(i, defaults[i])).collect();
        }
    }

    fn candidate(&self, pos: usize, discard: Tile) -> Cand {
        blank(
            Decision::Play(pos as u32 * 64 + discard as u32),
            Some(Move {
                acts: self.positions[pos].clone(),
                discard,
            }),
        )
    }

    /// Replace the field with every discard of the given positions.
    fn expand(&mut self, positions: &[usize]) {
        self.cands = positions
            .iter()
            .flat_map(|&i| self.discards[i].iter().map(move |&t| (i, t)))
            .map(|(i, t)| self.candidate(i, t))
            .collect();
        self.screening = false;
        self.version += 1;
    }

    /// What has to be played now. Results are accepted for this and the few fields before it.
    pub fn field(&mut self) -> Field {
        if self.sent.back().is_none_or(|f| f.version != self.version) {
            self.sent.push_back(Field {
                version: self.version,
                depth: self.depth,
                cands: self.cands.iter().filter(|c| c.alive).map(|c| c.decision).collect(),
            });
            if self.sent.len() > FIELDS_KEPT {
                self.sent.pop_front();
            }
        }
        self.sent.back().unwrap().clone()
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// Take over a field to play; this analyzer must have been made from the same view and options.
    pub fn set_field(&mut self, field: &Field) {
        self.work = field
            .cands
            .iter()
            .map(|&d| {
                let mv = match d {
                    Decision::Play(id) => self.positions.get((id / 64) as usize).map(|acts| Move {
                        acts: acts.clone(),
                        discard: (id % 64) as Tile,
                    }),
                    _ => None,
                };
                (d, mv)
            })
            .collect();
        self.work_version = Some(field.version);
        self.work_depth = field.depth;
    }

    /// One deal, played out for every candidate of the field taken over, in its order.
    pub fn evaluate(&mut self, seed: u64) -> Vec<Packed> {
        // Every candidate faces the same deal, so the deal (and the inference behind it) is made once.
        let deal = self.view.sample(&mut Rng::new(seed), &mut self.sv, &self.opts);
        let net = self.net.clone();
        let whole = Rng::new(seed ^ 0x51ED_270B_3F4A_91C7).below(WHOLE_EVERY) == 0;
        let ai = net.as_deref().map(|net| Ai {
            net,
            // A chance of winning the match needs the hand's real scores.
            horizon: if self.view.stakes.is_some() { 0 } else { self.opts.horizon },
            whole,
        });
        let depth = self.work_depth;
        self.work
            .iter()
            .map(|(d, mv)| {
                pack(&match mv {
                    Some(mv) => play_move_on(&self.view, deal.clone(), &mut self.sv, mv, depth, ai),
                    None => play_decision_on(&self.view, deal.clone(), &mut self.sv, *d, depth, ai),
                })
            })
            .collect()
    }

    /// Take in deals played for field `version`. Deals that miss a candidate now alive are dropped.
    pub fn absorb(&mut self, version: u32, deals: &[Vec<Packed>]) {
        let Some(field) = self.sent.iter().find(|f| f.version == version).cloned() else {
            return;
        };
        let place: std::collections::HashMap<Decision, usize> = field.cands.iter().enumerate().map(|(i, &d)| (d, i)).collect();
        for results in deals {
            if field.depth != self.depth || results.len() != field.cands.len() {
                return;
            }
            let at: Option<Vec<usize>> = self
                .cands
                .iter()
                .filter(|c| c.alive)
                .map(|c| place.get(&c.decision).copied())
                .collect();
            let Some(at) = at else { return };
            self.iter += 1;
            for (c, i) in self.cands.iter_mut().filter(|c| c.alive).zip(at) {
                let [gap, tie, win, flags] = results[i];
                c.scores.push(gap);
                c.tie += tie as i64;
                c.win += win as f64 / 1e6;
                c.next_takes += (flags >> 2 & 1) as u32;
                if flags >> 3 & 1 == 1 {
                    c.whole += 1;
                    c.finish += (flags & 1) as u32;
                    c.open += (flags >> 1 & 1) as u32;
                    if flags >> 4 > 0 {
                        c.last_draw[(flags >> 4) as usize - 1] += 1;
                    }
                }
            }
            if self.screening {
                self.screen();
            } else if self.iter >= self.next_prune {
                self.next_prune = self.iter + (self.iter / 16).max(4);
                self.prune();
                self.deepen();
            }
        }
    }

    pub fn step(&mut self, iterations: usize) {
        for _ in 0..iterations {
            if self.work_version != Some(self.version) {
                let field = self.field();
                self.set_field(&field);
            }
            let seed = self.seed.wrapping_add(self.iter.wrapping_mul(0x2545_F491_4F6C_DD1D));
            let results = self.evaluate(seed);
            self.absorb(self.version, &[results]);
        }
    }

    /// Switch the survivors to look-ahead simulations and start their statistics afresh,
    /// since scores from the two kinds of simulation are not comparable.
    fn deepen(&mut self) {
        // Near the end of a match the bot's habit of taking any finish is exactly what must be questioned.
        if !(self.opts.lookahead || decisive(&self.view)) || self.depth > 0 {
            return;
        }
        let alive = self.cands.iter().filter(|c| c.alive).count();
        let sampled = self.best_alive().map_or(0, |b| self.cands[b].scores.len());
        if alive > DEEP_FIELD || sampled < DEEP_AFTER {
            return;
        }
        self.depth = if self.view.deck_count <= 4 { 2 } else { 1 };
        self.cands.retain(|c| c.alive);
        for c in &mut self.cands {
            *c = blank(c.decision, c.mv.take());
        }
        self.version += 1;
    }

    /// Successive halving over all positions on the same deals, so that none is dropped unseen.
    fn screen(&mut self) {
        let n = self.cands.iter().map(|c| c.scores.len()).min().unwrap_or(0);
        if n < self.next_cut {
            return;
        }
        self.next_cut *= 2;
        self.version += 1;
        let field = self.cands.len();
        let keep = (if field > 500 { field / 3 } else { field / 2 }).max(SCREEN_KEEP);
        self.cands.sort_by(merit);
        let leader = self.cands[0].scores.clone();
        // Rank by how good each could still be, so a strong move that drew a few bad deals survives the cut.
        let mut ranked: Vec<(f64, Cand)> = self
            .cands
            .drain(..)
            .enumerate()
            .map(|(i, c)| {
                let (d, se) = paired(&c.scores, &leader);
                let could_be = if i > 0 && se.is_finite() {
                    d - SCREEN_DOUBT * se
                } else {
                    f64::NEG_INFINITY
                };
                (could_be, c)
            })
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
        self.cands = ranked.into_iter().map(|(_, c)| c).collect();
        self.cands.truncate(keep);
        if keep <= SCREEN_KEEP {
            let survivors: Vec<usize> = self
                .cands
                .iter()
                .filter_map(|c| match c.decision {
                    Decision::Play(id) => Some((id / 64) as usize),
                    _ => None,
                })
                .collect();
            self.expand(&survivors);
        }
    }

    fn best_alive(&self) -> Option<usize> {
        (0..self.cands.len())
            .filter(|&i| self.cands[i].alive)
            .min_by(|&a, &b| merit(&self.cands[a], &self.cands[b]))
    }

    fn prune(&mut self) {
        let Some(b) = self.best_alive() else { return };
        if self.cands[b].scores.len() < PRUNE_AFTER {
            return;
        }
        let alive = self.cands.iter().filter(|c| c.alive).count();
        if alive <= 3 {
            return;
        }
        let best = self.cands[b].scores.clone();
        let margin = self.margin(b);
        for (i, c) in self.cands.iter_mut().enumerate() {
            if i == b || !c.alive {
                continue;
            }
            let (d, se) = paired(&c.scores, &best);
            if d > 3.5 * se + margin {
                c.alive = false;
                self.version += 1;
            }
        }
    }

    /// Scores per point, or per mille of match chance when playing for the match.
    fn unit(&self) -> f64 {
        if self.view.stakes.is_some() {
            MATCH_SCALE / 1000.0
        } else {
            GAP_SCALE
        }
    }

    /// The difference not worth telling apart: a point, or a per mille of match chance. When the match
    /// is nearly lost or won, a thirtieth of what chance is left, since that is all there is to play for.
    fn margin(&self, best: usize) -> f64 {
        let c = &self.cands[best];
        if self.view.stakes.is_none() || c.scores.is_empty() {
            return self.unit();
        }
        let p = c.win / c.scores.len() as f64;
        self.unit() * (30.0 * p.min(1.0 - p)).clamp(0.1, 1.0)
    }

    pub fn report(&self) -> Report {
        let b = self.best_alive();
        let best_scores = b.map(|b| self.cands[b].scores.clone()).unwrap_or_default();
        // How much better than the leader each rival could still turn out to be.
        let (mut doubt, mut close) = (0f64, false);
        for (i, c) in self.cands.iter().enumerate() {
            if c.alive && Some(i) != b {
                let (d, se) = paired(&c.scores, &best_scores);
                doubt = doubt.max(SETTLE_SURE * se - d);
                close |= d < 2.0 * se;
            }
        }
        let unit = self.unit();
        let settled = !self.screening && best_scores.len() >= SETTLE_AFTER && b.is_some_and(|b| doubt < SETTLE_WITHIN * self.margin(b));
        let mut order: Vec<usize> = (0..self.cands.len()).collect();
        order.sort_by(|&a, &b| {
            let (x, y) = (&self.cands[a], &self.cands[b]);
            y.alive.cmp(&x.alive).then(merit(x, y))
        });
        let cands = order
            .into_iter()
            .take(REPORT_ROWS)
            .enumerate()
            .map(|(rank, i)| {
                let c = &self.cands[i];
                let n = c.scores.len().max(1) as f64;
                let whole = c.whole.max(1) as f64;
                CandReport {
                    decision: c.decision,
                    label: match &c.mv {
                        Some(mv) => move_label(mv, &self.view.rules),
                        None => label(&c.decision),
                    },
                    mean: mean(&c.scores) / unit,
                    gap: c.tie as f64 / n / GAP_SCALE,
                    diff_se: paired(&c.scores, &best_scores).1 / unit,
                    n: c.scores.len(),
                    p_finish: c.finish as f64 / whole,
                    p_open: c.open as f64 / whole,
                    p_next_takes: c.next_takes as f64 / n,
                    p_last_draw: c.last_draw.map(|k| k as f64 / whole),
                    whole: c.whole as usize,
                    p_win: self.view.stakes.map(|_| c.win / n),
                    alive: c.alive,
                    mv: c.mv.clone().filter(|_| rank < MOVES_SHOWN),
                }
            })
            .collect();
        Report {
            iterations: self.iter,
            candidates: cands,
            positions: self.positions.len(),
            screening: self.screening,
            truncated: self.truncated,
            deep: self.depth > 0,
            model: self.net.is_some(),
            settled,
            doubt: doubt.max(0.0) / unit,
            close,
        }
    }
}

/// Candidates in order of merit: mean cost first, mean score gap between equals.
fn merit(a: &Cand, b: &Cand) -> std::cmp::Ordering {
    let tie = |c: &Cand| c.tie as f64 / c.scores.len().max(1) as f64;
    mean(&a.scores).total_cmp(&mean(&b.scores)).then(tie(a).total_cmp(&tie(b)))
}

fn mean(v: &[i32]) -> f64 {
    if v.is_empty() {
        return f64::INFINITY;
    }
    v.iter().map(|&x| x as f64).sum::<f64>() / v.len() as f64
}

/// Mean and standard error of a - b over their common iterations (same deals: common random numbers).
fn paired(a: &[i32], b: &[i32]) -> (f64, f64) {
    let n = a.len().min(b.len());
    if n < 2 {
        return (0.0, f64::INFINITY);
    }
    let d: Vec<f64> = (0..n).map(|i| (a[i] - b[i]) as f64).collect();
    let m = d.iter().sum::<f64>() / n as f64;
    let var = d.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1) as f64;
    (m, (var / n as f64).sqrt())
}

/// Legal decisions for seat 0 in the current phase. Choices that lead to the same position are
/// merged, and each keeps only its most promising discards.
pub fn candidates(view: &View, sv: &mut Solver, opts: &Options) -> Vec<Decision> {
    let mut rng = Rng::new(1);
    let mut g = view.determinize(&mut rng);
    g.policy = [opts.policy; 4];
    let mut out = Vec::new();
    match view.phase {
        Phase::BeforeDraw => {
            if view.deck_count > 0 {
                out.push(Decision::Draw);
            }
            if let Some(t) = g.top
                && (!view.rules.take_must_use || g.can_use_taken(sv, 0, t))
            {
                out.push(Decision::Take);
            }
        }
        Phase::AfterDraw => {
            for (choice, after, end) in g.plan_positions(sv, 0, view.taken) {
                let discards: Vec<Option<Tile>> = match end {
                    Some(_) => vec![None],
                    None => after.best_discards(sv, 0, opts.max_discards).into_iter().map(Some).collect(),
                };
                out.extend(discards.into_iter().map(|discard| Decision::Act { choice, discard }));
            }
        }
    }
    out
}
