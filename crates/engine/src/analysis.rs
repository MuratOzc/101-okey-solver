use crate::features::{Moment, encode};
use crate::game::*;
use crate::meld::{Shape, Solver};
use crate::model::{model_act, play_out_to, play_out_with, written3};
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerView {
    pub discards: Vec<Tile>,
    pub known: Vec<Tile>,
    pub opened: Opened,
    pub open_value: i32,
    pub hand_len: Option<u8>,
    pub gosterge_bonus: bool,
}

impl Default for PlayerView {
    fn default() -> Self {
        PlayerView {
            discards: vec![],
            known: vec![],
            opened: Opened::No,
            open_value: 0,
            hand_len: None,
            gosterge_bonus: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    BeforeDraw,
    AfterDraw,
}

/// Everything the user has entered, from seat 0's point of view. Seat order is the play order:
/// 0 = me, 1 = right, 2 = across, 3 = left (whose discard I may take).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct View {
    pub rules: Rules,
    pub gosterge: Tile,
    pub hand: Vec<Tile>,
    pub players: Vec<PlayerView>,
    pub table: Vec<TableMeld>,
    #[serde(default)]
    pub pairs_added: [u8; 4],
    pub deck_count: u8,
    pub phase: Phase,
    pub taken: Option<Tile>,
    /// The match situation, when the player has entered it.
    #[serde(default)]
    pub stakes: Option<Stakes>,
}

impl View {
    pub fn left_top(&self) -> Option<Tile> {
        self.players.get(3).and_then(|p| p.discards.last().copied())
    }

    pub fn requirements(&self) -> (i32, u8) {
        let r = &self.rules;
        let mut meld = r.open_min;
        let mut pair = r.pair_min;
        if r.katlamali {
            for p in &self.players {
                match p.opened {
                    Opened::Melds => meld = meld.max(p.open_value + r.raise_meld_by),
                    Opened::Pairs => pair = pair.max(p.open_value as u8 + r.raise_pair_by),
                    Opened::No => {}
                }
            }
        }
        (meld, pair)
    }

    pub fn seen(&self) -> Counts {
        let mut c = [0u8; NIDS];
        let mut add = |t: Tile| c[t as usize] = c[t as usize].saturating_add(1);
        self.hand.iter().for_each(|&t| add(t));
        add(self.gosterge);
        for p in &self.players {
            p.discards.iter().for_each(|&t| add(t));
            p.known.iter().for_each(|&t| add(t));
        }
        for m in &self.table {
            m.shape.tiles().into_iter().for_each(&mut add);
        }
        c
    }

    pub fn unseen(&self) -> Counts {
        let s = self.seen();
        let mut u = [0u8; NIDS];
        for i in 0..NIDS {
            u[i] = 2u8.saturating_sub(s[i]);
        }
        u
    }

    pub fn problems(&self) -> Vec<String> {
        let mut v = Vec::new();
        let s = self.seen();
        for i in 0..NIDS {
            if s[i] > 2 {
                v.push(format!("{} {} kez girilmiş (en fazla 2)", name(i as Tile), s[i]));
            }
        }
        if self.players.len() != 4 {
            v.push("4 oyuncu olmalı".into());
        }
        v
    }

    pub fn hand_len_of(&self, p: usize) -> u8 {
        if let Some(n) = self.players[p].hand_len {
            return n;
        }
        let laid: usize = self
            .table
            .iter()
            .filter(|m| m.owner as usize == p)
            .map(|m| m.shape.len() as usize)
            .sum();
        self.rules.hand_size.saturating_sub(laid as u8)
    }

    pub fn determinize(&self, rng: &mut Rng) -> Game {
        let unseen = self.unseen();
        let mut pool = tiles_of(&unseen);
        rng.shuffle(&mut pool);
        let (meld_req, pair_req) = self.requirements();
        let mut seen = [0u8; NIDS];
        seen[self.gosterge as usize] += 1;
        for p in &self.players {
            p.discards.iter().for_each(|&t| seen[t as usize] += 1);
        }
        for m in &self.table {
            m.shape.tiles().into_iter().for_each(|t| seen[t as usize] += 1);
        }
        let mut players: Vec<Player> = Vec::with_capacity(4);
        for p in 0..4 {
            let pv = &self.players[p];
            let mut hand = [0u8; NIDS];
            if p == 0 {
                hand = counts_from(&self.hand);
            } else {
                for &t in &pv.known {
                    hand[t as usize] += 1;
                }
                let need = (self.hand_len_of(p) as usize).saturating_sub(pv.known.len());
                for _ in 0..need {
                    if let Some(t) = pool.pop() {
                        hand[t as usize] += 1;
                    }
                }
            }
            players.push(Player {
                hand,
                opened: pv.opened,
                gosterge_bonus: pv.gosterge_bonus,
                penalty: 0,
            });
        }
        pool.truncate(self.deck_count as usize);
        let top = match self.phase {
            Phase::BeforeDraw => self.left_top(),
            Phase::AfterDraw => None,
        };
        let mut piles = [[0u8; NIDS]; 4];
        let mut last = [None; 4];
        for (p, pv) in self.players.iter().enumerate().take(4) {
            pv.discards.iter().for_each(|&t| piles[p][t as usize] += 1);
            last[p] = pv.discards.last().copied();
        }
        Game {
            rules: self.rules.clone(),
            gosterge: self.gosterge,
            players: players.try_into().expect("4 players"),
            deck: pool,
            table: Table {
                melds: self.table.clone(),
                pairs_added: self.pairs_added,
            },
            top,
            current: 0,
            meld_req,
            pair_req,
            seen,
            piles,
            last,
            last_draw: None,
            log: None,
            policy: [Policy::default(); 4],
            stakes: self.stakes,
        }
    }
}

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
    /// Expected own (team) score for this hand.
    pub own: f64,
    pub diff_se: f64,
    pub n: usize,
    pub p_finish: f64,
    pub p_open: f64,
    pub p_opp_finish: f64,
    pub p_deckout: f64,
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

/// One candidate's result on one deal: gap, tie, own, chance of winning per million, flags.
pub type Packed = [i32; 5];

fn pack(r: &RolloutResult) -> Packed {
    let flags = r.finish as i32
        | (r.opened as i32) << 1
        | (r.opp_finish as i32) << 2
        | (r.deckout as i32) << 3
        | (r.next_takes as i32) << 4
        | r.last_draw.map_or(0, |p| p as i32 + 1) << 5
        | (r.whole as i32) << 8;
    [r.gap, r.tie, r.own, (r.win as f64 * 1e6).round() as i32, flags]
}

struct Cand {
    decision: Decision,
    mv: Option<Move>,
    scores: Vec<i32>,
    /// Summed score gap, used only to order candidates whose `scores` are level.
    tie: i64,
    win: f64,
    own: i64,
    finish: u32,
    open: u32,
    opp_finish: u32,
    deckout: u32,
    next_takes: u32,
    last_draw: [u32; 4],
    /// Deals played to the end of the hand; the counts above come from these alone.
    whole: u32,
    alive: bool,
    prior: Option<f32>,
    rated: bool,
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
    prior_weight: f64,
    own_turns: bool,
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
        own: 0,
        finish: 0,
        open: 0,
        opp_finish: 0,
        deckout: 0,
        next_takes: 0,
        last_draw: [0; 4],
        whole: 0,
        alive: true,
        prior: None,
        rated: false,
    }
}

/// How the analyzer searches and what it assumes about the other players.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// Try alternative openings, holding melds back and taking opponents' okeys, not just "lay everything".
    pub rich_moves: bool,
    /// How every simulated player (me in later turns, and the opponents) is assumed to play.
    pub policy: Policy,
    pub max_discards: usize,
    /// Deal the hidden tiles so that they fit what the opponents have done so far,
    /// instead of uniformly at random.
    pub infer: bool,
    /// Consider every legal move (all arrangements, lay-offs, okey swaps, discards) instead of a few plans.
    pub exhaustive: bool,
    /// Once few candidates remain, simulate my own later turns by search instead of by the bot.
    /// Off by default: in the arena it cost ten times the work and changed nothing (±8 points/hand).
    pub lookahead: bool,
    /// With the network playing my later turns: stop each simulation after this many of them and take
    /// what the network expects from there. 0 plays every simulation to the end of the hand.
    pub horizon: u8,
}

/// How the network plays my later turns inside one simulation.
#[derive(Clone, Copy)]
struct Ai<'a> {
    net: &'a Net,
    horizon: u8,
    /// Play on past the horizon to the end of the hand, for what only a finished hand can tell.
    whole: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            rich_moves: true,
            policy: Policy::FINISHER,
            max_discards: 12,
            infer: true,
            exhaustive: true,
            lookahead: false,
            horizon: 1,
        }
    }
}

/// How much less likely a deal is when an opponent "would have" kept a tile they threw away…
const DISCARD_FITS: f32 = 0.35;
/// …or is sitting on an opening they would normally have laid down.
const UNOPENED_BUT_READY: f32 = 0.2;
const INFER_TRIES: usize = 12;
const DISCARDS_REMEMBERED: usize = 4;
/// Whether `d` would complete a run or a group with two tiles of `hand`.
fn completes(hand: &Counts, d: Tile) -> bool {
    if d == JOKER {
        return false;
    }
    let (c, n) = (color(d), num(d) as i32);
    let has = |k: i32| (1..=13).contains(&k) && hand[id(c, k as u8) as usize] > 0;
    let run = (has(n - 2) && has(n - 1)) || (has(n - 1) && has(n + 1)) || (has(n + 1) && has(n + 2));
    let mates = (0..NCOLORS).filter(|&c2| c2 != c && hand[id(c2, n as u8) as usize] > 0).count();
    run || mates >= 2
}

impl View {
    /// How well a dealt-out game fits the opponents' visible behaviour, in (0, 1].
    /// Kept soft on purpose: people bluff and hold tiles back.
    fn plausibility(&self, g: &Game, sv: &mut Solver) -> f32 {
        let mut w = 1.0f32;
        for o in 1..4usize {
            let pv = &self.players[o];
            let hand = &g.players[o].hand;
            for &d in pv.discards.iter().rev().take(DISCARDS_REMEMBERED) {
                if completes(hand, d) {
                    w *= DISCARD_FITS;
                }
            }
            if pv.opened == Opened::No && !pv.discards.is_empty() && !g.should_hold(o as u8) {
                let ready = g.open_search(sv, hand, None).is_some_and(|p| p.open_value(&g.rules) >= g.meld_req)
                    || g.pairs_ok(o as u8, None).is_some();
                if ready {
                    w *= UNOPENED_BUT_READY;
                }
            }
        }
        w
    }

    /// A deal of the hidden tiles, optionally drawn in proportion to how plausible it is.
    pub fn sample(&self, rng: &mut Rng, sv: &mut Solver, opts: &Options) -> Game {
        let mut g = self.determinize(rng);
        g.policy = [opts.policy; 4];
        if opts.infer {
            for _ in 0..INFER_TRIES {
                let w = self.plausibility(&g, sv);
                if (rng.below(10_000) as f32) < w * 10_000.0 {
                    break;
                }
                g = self.determinize(rng);
                g.policy = [opts.policy; 4];
            }
        }
        g
    }
}

pub fn label(d: &Decision) -> String {
    match d {
        Decision::Draw => "Desteden çek".into(),
        Decision::Take => "Yandan al".into(),
        Decision::Play(_) => "Hamle".into(),
        Decision::Act { choice, discard } => {
            let lay_note = |l: &LayStyle| match l {
                LayStyle::Nothing => ", başka bir şey indirme",
                LayStyle::AllSafe => ", okeyi seriye koyma",
                LayStyle::All => "",
            };
            let what = match choice {
                OpenChoice::Stay => "Açma".to_string(),
                OpenChoice::OpenMelds(style, lay) => {
                    let how = match style {
                        OpenStyle::MaxValue => "en yüksek puanla",
                        OpenStyle::MaxCover => "en çok taşla",
                        OpenStyle::Minimal => "en az taşla",
                        OpenStyle::SafeOkey => "okey seride değil",
                        OpenStyle::NoOkey => "okeyi elde tut",
                    };
                    format!("Perle aç ({how}{})", lay_note(lay))
                }
                OpenChoice::OpenPairs(lay) => format!("Çiftle aç{}", lay_note(lay)),
                OpenChoice::Lay(LayStyle::All) => "İndir / işle".to_string(),
                OpenChoice::Lay(LayStyle::AllSafe) => "İndir / işle (okeyi seriye koyma)".to_string(),
                OpenChoice::Lay(LayStyle::Nothing) => "Hiçbir şey indirme".to_string(),
                OpenChoice::Finish => "Bitir".to_string(),
            };
            match discard {
                Some(t) => format!("{what}, {} at", name(*t)),
                None => format!("{what} (el biter)"),
            }
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
            prior_weight: 0.0,
            own_turns: false,
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
        } else if opts.exhaustive && me.view.phase == Phase::AfterDraw {
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

    /// The network's opinion of a candidate counts like `weight` simulated deals; with `own_turns`
    /// it also plays my later turns inside every simulation.
    pub fn use_model(&mut self, net: Arc<Net>, weight: f64, own_turns: bool) {
        if !model_fits(&self.view, net.partners) {
            return;
        }
        self.net = Some(net);
        // The network's own opinion is in points, which a chance of winning the match cannot be mixed with.
        self.prior_weight = if self.view.stakes.is_some() { 0.0 } else { weight };
        self.own_turns = own_turns;
        self.rate();
    }

    fn rate(&mut self) {
        let Some(net) = self.net.clone() else { return };
        if self.prior_weight <= 0.0 || self.cands.iter().all(|c| c.rated) {
            return;
        }
        let mut base = self.view.determinize(&mut Rng::new(1));
        base.policy = [self.opts.policy; 4];
        let taken = self.view.taken;
        for c in self.cands.iter_mut().filter(|c| !c.rated) {
            c.rated = true;
            let mut g = base.clone();
            let sv = &mut self.sv;
            c.prior = match (&c.mv, c.decision) {
                (Some(mv), _) => g.play_move(0, mv, taken).ok().map(|end| settle(&mut g, sv, &net, end)),
                (None, Decision::Draw) => Some(written3(&g, 0) as f32 + net.value(&encode(&g, sv, 0, Moment::Draw, net.full()))),
                (None, Decision::Take) if g.top.is_some() => {
                    let t = g.take_top(0);
                    Some(written3(&g, 0) as f32 + net.value(&encode(&g, sv, 0, Moment::Take(t), net.full())))
                }
                (None, Decision::Act { choice, discard }) => g.apply_choice(sv, 0, choice, taken).ok().map(|end| {
                    if end.is_none() {
                        let t = discard
                            .filter(|&t| g.players[0].hand[t as usize] > 0)
                            .unwrap_or_else(|| g.bot_discard(sv, 0));
                        g.discard(0, t);
                    }
                    settle(&mut g, sv, &net, end)
                }),
                _ => None,
            };
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
            // Throwing an işlek tile or an okey costs a flat penalty: never worth it while a free discard exists.
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
        self.rate();
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
        let net = self.net.clone().filter(|_| self.own_turns);
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
                let [gap, tie, own, win, flags] = results[i];
                c.scores.push(gap);
                c.tie += tie as i64;
                c.win += win as f64 / 1e6;
                c.next_takes += (flags >> 4 & 1) as u32;
                if flags >> 8 & 1 == 1 {
                    c.whole += 1;
                    c.own += own as i64;
                    c.finish += (flags & 1) as u32;
                    c.open += (flags >> 1 & 1) as u32;
                    c.opp_finish += (flags >> 2 & 1) as u32;
                    c.deckout += (flags >> 3 & 1) as u32;
                    if flags >> 5 & 7 > 0 {
                        c.last_draw[(flags >> 5 & 7) as usize - 1] += 1;
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
        self.rate();
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
        let w = self.prior_weight;
        self.cands.sort_by(|a, b| merit(a, b, w));
        let leader = self.cands[0].scores.clone();
        // Rank by how good each could still be, so a strong move that drew a few bad deals survives the cut.
        let mut ranked: Vec<(f64, Cand)> = self
            .cands
            .drain(..)
            .map(|c| {
                let (d, se) = paired(&c.scores, &leader);
                (if se.is_finite() { d - SCREEN_DOUBT * se } else { f64::NEG_INFINITY }, c)
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
            .min_by(|&a, &b| merit(&self.cands[a], &self.cands[b], self.prior_weight))
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
        let (best_prior, w) = (self.cands[b].prior, self.prior_weight);
        let margin = self.margin(b);
        for (i, c) in self.cands.iter_mut().enumerate() {
            if i == b || !c.alive {
                continue;
            }
            let (mut d, mut se) = paired(&c.scores, &best);
            if let (Some(p), Some(q)) = (c.prior, best_prior)
                && w > 0.0
                && se.is_finite()
            {
                let n = c.scores.len().min(best.len()) as f64;
                d = (n * d + w * (p - q) as f64) / (n + w);
                se *= (n / (n + w)).sqrt();
            }
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
            y.alive.cmp(&x.alive).then(merit(x, y, self.prior_weight))
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
                    mean: est(c, self.prior_weight) / unit,
                    gap: c.tie as f64 / n / GAP_SCALE,
                    own: c.own as f64 / whole,
                    diff_se: paired(&c.scores, &best_scores).1 / unit,
                    n: c.scores.len(),
                    p_finish: c.finish as f64 / whole,
                    p_open: c.open as f64 / whole,
                    p_opp_finish: c.opp_finish as f64 / whole,
                    p_deckout: c.deckout as f64 / whole,
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
            model: self.net.is_some() && self.own_turns,
            settled,
            doubt: doubt.max(0.0) / unit,
            close,
        }
    }
}

fn shape_short(sh: &Shape) -> String {
    let okey = if sh.jokers() > 0 { "+okey" } else { "" };
    match *sh {
        Shape::Run { color, lo, hi, .. } => format!("{}{}-{}{okey}", COLOR_SHORT[color as usize], lo, if hi == 14 { 1 } else { hi }),
        Shape::Group { num, .. } => format!("{num}×{}{okey}", sh.len()),
        Shape::Pair { tile, .. } if tile == JOKER => "okey çifti".to_string(),
        Shape::Pair { tile, joker } => format!("{} çift{}", short(tile), if joker { "+okey" } else { "" }),
        Shape::GostergePair { other, .. } => format!("gösterge+{}", short(other)),
    }
}

/// A one-line summary of an explicit move that names the melds, so different arrangements read differently.
pub fn move_label(mv: &Move, rules: &Rules) -> String {
    let mut melds = Vec::new();
    let mut value = 0;
    let mut laid = Vec::new();
    let (mut swaps, mut gifts) = (0, 0);
    for act in &mv.acts {
        match act {
            Act::Meld(s) => {
                melds.push(shape_short(s));
                value += s.open_value(rules);
            }
            Act::Layoff { tile, .. } => laid.push(short(*tile)),
            Act::Swap(_) => swaps += 1,
            Act::Gift { .. } => gifts += 1,
        }
    }
    let mut parts = Vec::new();
    if !melds.is_empty() {
        let total = if value > 0 { format!(" ({value})") } else { String::new() };
        parts.push(format!("İndir {}{total}", melds.join(", ")));
    }
    if swaps > 0 {
        parts.push("okey al".to_string());
    }
    if !laid.is_empty() {
        parts.push(format!("işle {}", laid.join(", ")));
    }
    if gifts > 0 {
        parts.push(format!("{gifts} çift işle"));
    }
    if parts.is_empty() {
        parts.push("İndirme yok".to_string());
    }
    format!("{} · {} at", parts.join(" · "), name(mv.discard))
}

/// Candidates in order of merit: mean cost first, mean score gap between equals.
fn merit(a: &Cand, b: &Cand, weight: f64) -> std::cmp::Ordering {
    let tie = |c: &Cand| c.tie as f64 / c.scores.len().max(1) as f64;
    est(a, weight).total_cmp(&est(b, weight)).then(tie(a).total_cmp(&tie(b)))
}

fn est(c: &Cand, weight: f64) -> f64 {
    match c.prior {
        Some(p) if weight > 0.0 => (c.scores.iter().map(|&x| x as f64).sum::<f64>() + weight * p as f64) / (c.scores.len() as f64 + weight),
        _ => mean(&c.scores),
    }
}

fn settle(g: &mut Game, sv: &mut Solver, net: &Net, end: Option<Outcome>) -> f32 {
    match end {
        Some(end) => g.gap3(&g.scores(&end), 0) as f32,
        None => {
            g.current = 1;
            written3(g, 0) as f32 + net.value(&encode(g, sv, 0, Moment::After, net.full()))
        }
    }
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

/// Gaps are stored times this so the three-opponent average stays an integer.
const GAP_SCALE: f64 = 3.0;
/// The chance of losing the match is stored times this, fine enough for a match that is nearly lost.
const MATCH_SCALE: f64 = 3_000_000.0;

pub struct RolloutResult {
    /// GAP_SCALE × (my score − opponents' average); team against team when partnered.
    /// With a known match situation: GAP_SCALE × the chance of losing the match, per mille.
    pub gap: i32,
    /// Chance of winning the match in this outcome (0 when no match situation is known).
    pub win: f32,
    /// The plain score gap, for ordering outcomes that are level on `gap`.
    pub tie: i32,
    pub own: i32,
    pub finish: bool,
    pub opened: bool,
    pub opp_finish: bool,
    pub deckout: bool,
    pub next_takes: bool,
    /// Who drew the deck's last tile, counted from me.
    pub last_draw: Option<u8>,
    /// The hand was played to its end, so everything but `gap` and `tie` describes a real ending.
    pub whole: bool,
}

pub fn rollout(view: &View, sv: &mut Solver, d: Decision, seed: u64, opts: &Options, depth: u8) -> RolloutResult {
    let g = view.sample(&mut Rng::new(seed), sv, opts);
    play_decision_on(view, g, sv, d, depth, None)
}

fn play_decision_on(view: &View, mut g: Game, sv: &mut Solver, d: Decision, depth: u8, ai: Option<Ai>) -> RolloutResult {
    let mut out = None;
    let mut rest = depth;
    match d {
        Decision::Draw => match g.draw(0) {
            Some(_) => {
                out = match ai {
                    Some(ai) => model_act(&mut g, sv, ai.net, None),
                    None if depth > 0 => g.lookahead_act(sv, 0, None, depth - 1),
                    None => g.bot_act(sv, 0, None),
                };
                rest = depth.saturating_sub(1);
            }
            None => out = Some(Outcome::DeckOut),
        },
        Decision::Take if g.deck.is_empty() => out = Some(Outcome::DeckOut),
        Decision::Take => {
            let t = g.take_top(0);
            out = match ai {
                Some(ai) => model_act(&mut g, sv, ai.net, Some(t)),
                None if depth > 0 => g.lookahead_act(sv, 0, Some(t), depth - 1),
                None => g.bot_act(sv, 0, Some(t)),
            };
            rest = depth.saturating_sub(1);
        }
        Decision::Play(_) => {
            let t = g.bot_discard(sv, 0);
            g.discard(0, t);
        }
        Decision::Act { choice, discard } => match g.apply_choice(sv, 0, choice, view.taken) {
            Ok(Some(o)) => out = Some(o),
            Ok(None) => {
                let t = discard.unwrap_or_else(|| g.bot_discard(sv, 0));
                if g.players[0].hand[t as usize] > 0 {
                    g.discard(0, t);
                } else {
                    let t = g.bot_discard(sv, 0);
                    g.discard(0, t);
                }
            }
            Err(()) => {
                let t = g.bot_discard(sv, 0);
                g.discard(0, t);
            }
        },
    }
    conclude(view, g, out, sv, rest, ai)
}

/// One simulated hand after seat 0 plays an explicit move.
pub fn rollout_move(view: &View, sv: &mut Solver, mv: &Move, seed: u64, opts: &Options, depth: u8) -> RolloutResult {
    let g = view.sample(&mut Rng::new(seed), sv, opts);
    play_move_on(view, g, sv, mv, depth, None)
}

fn play_move_on(view: &View, mut g: Game, sv: &mut Solver, mv: &Move, depth: u8, ai: Option<Ai>) -> RolloutResult {
    let out = match g.play_move(0, mv, view.taken) {
        Ok(o) => o,
        Err(()) => {
            let t = g.bot_discard(sv, 0);
            g.discard(0, t);
            None
        }
    };
    conclude(view, g, out, sv, depth, ai)
}

/// Play the hand out from just after seat 0's turn, and score it.
fn conclude(view: &View, mut g: Game, out: Option<Outcome>, sv: &mut Solver, depth: u8, ai: Option<Ai>) -> RolloutResult {
    let mut next_takes = false;
    let mut expected = None;
    let o = match out {
        Some(o) => o,
        None => {
            g.current = 1;
            next_takes = g.bot_wants_take(sv, 1);
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
                            own: 0,
                            finish: false,
                            opened: false,
                            opp_finish: false,
                            deckout: false,
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
    let team = view.rules.partners;
    let own = if team { s[0] + s[2] } else { s[0] };
    let tie = expected.unwrap_or_else(|| g.gap3(&s, 0));
    let loss = g.stakes.map(|st| st.loss(&s, team));
    let gap = loss.map_or(tie, |l| (l * MATCH_SCALE).round() as i32);
    let win = loss.map_or(0.0, |l| 1.0 - l as f32);
    let (finish, opp_finish) = match o {
        Outcome::Finished(f) => {
            let mine = f.player == 0 || (team && f.player == 2);
            (mine, !mine)
        }
        Outcome::DeckOut => (false, false),
    };
    RolloutResult {
        gap,
        win,
        tie,
        own,
        finish,
        opened: g.players[0].opened != Opened::No || matches!(o, Outcome::Finished(f) if f.player == 0),
        opp_finish,
        deckout: o == Outcome::DeckOut,
        next_takes,
        last_draw: g.last_draw,
        // Hands that happen to end inside the horizon must not be counted more often than the rest.
        whole: ai.is_none_or(|a| a.whole || a.horizon == 0),
    }
}

/// The physical steps of an explicit move, in order, for showing to the player.
pub fn describe_move(view: &View, mv: &Move) -> Vec<Step> {
    let mut g = view.determinize(&mut Rng::new(3));
    g.log = Some(Vec::new());
    let _ = g.play_move(0, mv, view.taken);
    g.log.take().unwrap_or_default()
}

/// The physical steps seat 0 performs for `d`, in order, for showing to the player.
pub fn describe(view: &View, d: Decision) -> Vec<Step> {
    let Decision::Act { choice, discard } = d else { return Vec::new() };
    let mut sv = Solver::new(&view.rules);
    let mut g = view.determinize(&mut Rng::new(3));
    g.log = Some(Vec::new());
    if let Ok(None) = g.apply_choice(&mut sv, 0, choice, view.taken)
        && let Some(t) = discard.filter(|&t| g.players[0].hand[t as usize] > 0)
    {
        g.discard(0, t);
    }
    g.log.take().unwrap_or_default()
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
            let mut seen_positions: Vec<String> = Vec::new();
            for choice in Game::plan_choices(g.players[0].opened, opts.rich_moves) {
                let mut g2 = g.clone();
                let Ok(res) = g2.apply_choice(sv, 0, choice, view.taken) else {
                    continue;
                };
                let key = format!("{:?}|{:?}|{}|{:?}", g2.players[0].hand, g2.table.melds, g2.players[0].penalty, res);
                if seen_positions.contains(&key) {
                    continue;
                }
                seen_positions.push(key);
                match res {
                    Some(_) => out.push(Decision::Act { choice, discard: None }),
                    None => {
                        let mut scores = g2.discard_scores(sv, 0);
                        scores.sort_by_key(|&(_, s)| s);
                        for (t, _) in scores.into_iter().take(opts.max_discards) {
                            out.push(Decision::Act { choice, discard: Some(t) });
                        }
                    }
                }
            }
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Out {
    pub tile: Tile,
    pub unseen: u8,
    pub open_value: i32,
    pub pairs: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HandReport {
    pub meld_req: i32,
    pub pair_req: u8,
    pub open_value: i32,
    pub melds: Vec<Vec<Tile>>,
    pub rest: Vec<Tile>,
    pub pairs: u8,
    pub unseen: Vec<u8>,
    pub unseen_total: u32,
    pub outs: Vec<Out>,
    pub problems: Vec<String>,
}

pub fn hand_report(view: &View) -> HandReport {
    let mut sv = Solver::new(&view.rules);
    let (meld_req, pair_req) = view.requirements();
    let hand = counts_from(&view.hand);
    let g = view.determinize(&mut Rng::new(7));
    let gost = view.players[0].gosterge_bonus.then_some(view.gosterge);
    let plan = g.open_search(&mut sv, &hand, None).unwrap_or_default();
    let open_value = plan.open_value(&view.rules);
    let pairs = pair_plan(&hand, &view.rules, gost).pairs.len() as u8;
    let unseen = view.unseen();
    let mut outs = Vec::new();
    for t in 0..NIDS as Tile {
        if unseen[t as usize] == 0 {
            continue;
        }
        let mut h = hand;
        h[t as usize] += 1;
        let v = g.open_search(&mut sv, &h, None).map_or(0, |p| p.open_value(&view.rules));
        let pr = pair_plan(&h, &view.rules, gost).pairs.len() as u8;
        if v > open_value || pr > pairs {
            outs.push(Out {
                tile: t,
                unseen: unseen[t as usize],
                open_value: v,
                pairs: pr,
            });
        }
    }
    outs.sort_by_key(|o| std::cmp::Reverse(o.open_value));
    HandReport {
        meld_req,
        pair_req,
        open_value,
        melds: plan.melds.iter().map(Shape::tiles).collect(),
        rest: plan.rest.clone(),
        pairs,
        unseen: unseen.to_vec(),
        unseen_total: total(&unseen),
        outs,
        problems: view.problems(),
    }
}

/// Seat 0's view of a live game: public information plus its own hand.
pub fn public_view(g: &Game, phase: Phase, taken: Option<Tile>) -> View {
    let mut players = vec![PlayerView::default(); 4];
    for p in 0..4 {
        players[p].opened = g.players[p].opened;
        players[p].hand_len = Some(g.hand_len(p as u8) as u8);
        let mut pile = g.piles[p];
        let last = g.last[p].filter(|&t| pile[t as usize] > 0);
        if let Some(t) = last {
            pile[t as usize] -= 1;
        }
        players[p].discards = tiles_of(&pile);
        players[p].discards.extend(last);
    }
    players[0].gosterge_bonus = g.players[0].gosterge_bonus;
    View {
        rules: g.rules.clone(),
        gosterge: g.gosterge,
        hand: tiles_of(&g.players[0].hand),
        players,
        table: g.table.melds.clone(),
        pairs_added: g.table.pairs_added,
        deck_count: g.deck.len() as u8,
        phase,
        taken,
        stakes: g.stakes,
    }
}
