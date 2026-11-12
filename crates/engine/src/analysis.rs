use crate::features::{Moment, encode};
use crate::game::*;
use crate::meld::{Shape, Solver};
use crate::model::{model_act, play_out_with, written3};
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
    /// Chance of winning the match, when the match situation is known. `mean` is then the chance
    /// of losing it, per mille, instead of a score gap.
    #[serde(default)]
    pub p_win: Option<f64>,
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
}

/// Look-ahead costs roughly ten plain simulations each, so it starts only when this few candidates are left.
const DEEP_FIELD: usize = 6;
const DEEP_AFTER: usize = 24;

const POSITION_CAP: usize = 20_000;
/// Screening halves the field until this many positions remain; then every discard of each is compared.
const SCREEN_KEEP: usize = 12;
/// With no more moves than this, skip screening: every move competes under the careful pruning rule.
const SCREEN_ABOVE_MOVES: usize = 300;
const FIRST_CUT: usize = 6;
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
}

impl Default for Options {
    fn default() -> Self {
        Options {
            rich_moves: true,
            policy: Policy::TUNED,
            max_discards: 12,
            infer: true,
            exhaustive: true,
            lookahead: false,
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
        };
        // Playing for the match, what decides is finish-or-wait and which tile to give up, including
        // tiles that cost a penalty; a winning line may come up on one deal in fifty, so the field has to
        // stay small enough to replay every candidate on every deal. Plans with every discard do that.
        let plans = Options {
            max_discards: NIDS,
            ..opts
        };
        if me.view.stakes.is_some() && me.view.phase == Phase::AfterDraw {
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
        // A network knows only the plain score gap under the default rules, alone or in teams as it was trained.
        let trained = Rules {
            partners: net.partners,
            ..Rules::default()
        };
        if self.view.stakes.is_some() || self.view.rules != trained {
            return;
        }
        self.net = Some(net);
        self.prior_weight = weight;
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
        self.rate();
    }

    /// One more deal, played out for every live candidate.
    fn sample_once(&mut self) {
        let seed = self.seed.wrapping_add(self.iter.wrapping_mul(0x2545_F491_4F6C_DD1D));
        self.iter += 1;
        // Every candidate faces the same deal, so the deal (and the inference behind it) is made once.
        let deal = self.view.sample(&mut Rng::new(seed), &mut self.sv, &self.opts);
        let ai = self.net.clone().filter(|_| self.own_turns);
        for c in self.cands.iter_mut().filter(|c| c.alive) {
            let r = match &c.mv {
                Some(mv) => play_move_on(&self.view, deal.clone(), &mut self.sv, mv, self.depth, ai.as_deref()),
                None => play_decision_on(&self.view, deal.clone(), &mut self.sv, c.decision, self.depth, ai.as_deref()),
            };
            c.scores.push(r.gap);
            c.tie += r.tie as i64;
            c.win += r.win as f64;
            c.own += r.own as i64;
            c.finish += r.finish as u32;
            c.open += r.opened as u32;
            c.opp_finish += r.opp_finish as u32;
            c.deckout += r.deckout as u32;
            c.next_takes += r.next_takes as u32;
            if let Some(p) = r.last_draw {
                c.last_draw[p as usize] += 1;
            }
        }
    }

    pub fn step(&mut self, iterations: usize) {
        self.rate();
        for _ in 0..iterations {
            self.sample_once();
            if self.screening {
                self.screen();
            }
        }
        if !self.screening {
            self.prune();
            self.deepen();
        }
    }

    /// Switch the survivors to look-ahead simulations and start their statistics afresh,
    /// since scores from the two kinds of simulation are not comparable.
    fn deepen(&mut self) {
        // Near the end of a match the bot's habit of taking any finish is exactly what must be questioned.
        let decisive = self.view.stakes.is_some_and(|s| s.hands_left <= 1);
        if !(self.opts.lookahead || decisive) || self.depth > 0 {
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
        // A report must never go out with empty statistics.
        self.sample_once();
        self.sample_once();
    }

    /// Successive halving over all positions on the same deals, so that none is dropped unseen.
    fn screen(&mut self) {
        let n = self.cands.iter().map(|c| c.scores.len()).min().unwrap_or(0);
        if n < self.next_cut {
            return;
        }
        self.next_cut *= 2;
        let field = self.cands.len();
        let keep = (if field > 500 { field / 3 } else { field / 2 }).max(SCREEN_KEEP);
        let w = self.prior_weight;
        self.cands.sort_by(|a, b| merit(a, b, w));
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
        if self.cands[b].scores.len() < 12 {
            return;
        }
        let alive = self.cands.iter().filter(|c| c.alive).count();
        if alive <= 3 {
            return;
        }
        let best = self.cands[b].scores.clone();
        let (best_prior, w) = (self.cands[b].prior, self.prior_weight);
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
            if d > 3.5 * se + GAP_SCALE {
                c.alive = false;
            }
        }
    }

    pub fn report(&self) -> Report {
        let b = self.best_alive();
        let best_scores = b.map(|b| self.cands[b].scores.clone()).unwrap_or_default();
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
                CandReport {
                    decision: c.decision,
                    label: match &c.mv {
                        Some(mv) => move_label(mv, &self.view.rules),
                        None => label(&c.decision),
                    },
                    mean: est(c, self.prior_weight) / GAP_SCALE,
                    own: c.own as f64 / n,
                    diff_se: paired(&c.scores, &best_scores).1 / GAP_SCALE,
                    n: c.scores.len(),
                    p_finish: c.finish as f64 / n,
                    p_open: c.open as f64 / n,
                    p_opp_finish: c.opp_finish as f64 / n,
                    p_deckout: c.deckout as f64 / n,
                    p_next_takes: c.next_takes as f64 / n,
                    p_last_draw: c.last_draw.map(|k| k as f64 / n),
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
}

pub fn rollout(view: &View, sv: &mut Solver, d: Decision, seed: u64, opts: &Options, depth: u8) -> RolloutResult {
    let g = view.sample(&mut Rng::new(seed), sv, opts);
    play_decision_on(view, g, sv, d, depth, None)
}

fn play_decision_on(view: &View, mut g: Game, sv: &mut Solver, d: Decision, depth: u8, ai: Option<&Net>) -> RolloutResult {
    let mut out = None;
    let mut rest = depth;
    match d {
        Decision::Draw => match g.draw(0) {
            Some(_) => {
                out = match ai {
                    Some(net) => model_act(&mut g, sv, net, None),
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
                Some(net) => model_act(&mut g, sv, net, Some(t)),
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

fn play_move_on(view: &View, mut g: Game, sv: &mut Solver, mv: &Move, depth: u8, ai: Option<&Net>) -> RolloutResult {
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
fn conclude(view: &View, mut g: Game, out: Option<Outcome>, sv: &mut Solver, depth: u8, ai: Option<&Net>) -> RolloutResult {
    let mut next_takes = false;
    let o = match out {
        Some(o) => o,
        None => {
            g.current = 1;
            next_takes = g.bot_wants_take(sv, 1);
            if let Some(net) = ai {
                play_out_with(&mut g, sv, net, [true, false, false, false])
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
    let tie = g.gap3(&s, 0);
    let loss = g.stakes.map(|st| st.loss(&s, team));
    let gap = loss.map_or(tie, |l| (l * 3000.0).round() as i32);
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
    let mut disc = g.seen;
    disc[g.gosterge as usize] -= 1;
    for t in g.table.tiles() {
        disc[t as usize] -= 1;
    }
    let mut players = vec![PlayerView::default(); 4];
    for p in 0..4 {
        players[p].opened = g.players[p].opened;
        players[p].hand_len = Some(g.hand_len(p as u8) as u8);
    }
    players[0].gosterge_bonus = g.players[0].gosterge_bonus;
    let top = g.top.filter(|_| phase == Phase::BeforeDraw);
    if let Some(t) = top {
        disc[t as usize] -= 1;
    }
    players[1].discards = tiles_of(&disc);
    players[3].discards.extend(top);
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

/// Combine reports from independent analyzers of the same position (one per worker thread).
pub fn merge_reports(reports: &[Report]) -> Report {
    let mut out: Vec<CandReport> = Vec::new();
    for r in reports {
        for c in &r.candidates {
            let Some(m) = out.iter_mut().find(|m| m.decision == c.decision) else {
                let mut first = c.clone();
                first.diff_se = if c.diff_se.is_finite() {
                    (c.diff_se * c.n as f64).powi(2)
                } else {
                    f64::INFINITY
                };
                out.push(first);
                continue;
            };
            let (a, b) = (m.n as f64, c.n as f64);
            let total = (a + b).max(1.0);
            let mix = |x: f64, y: f64| (x * a + y * b) / total;
            m.mean = if c.n == 0 {
                m.mean
            } else if m.n == 0 {
                c.mean
            } else {
                mix(m.mean, c.mean)
            };
            m.own = mix(m.own, c.own);
            m.p_finish = mix(m.p_finish, c.p_finish);
            m.p_open = mix(m.p_open, c.p_open);
            m.p_opp_finish = mix(m.p_opp_finish, c.p_opp_finish);
            m.p_deckout = mix(m.p_deckout, c.p_deckout);
            m.p_next_takes = mix(m.p_next_takes, c.p_next_takes);
            for (x, &y) in m.p_last_draw.iter_mut().zip(&c.p_last_draw) {
                *x = mix(*x, y);
            }
            m.p_win = m.p_win.zip(c.p_win).map(|(x, y)| mix(x, y));
            m.diff_se += if c.diff_se.is_finite() {
                (c.diff_se * b).powi(2)
            } else {
                f64::INFINITY
            };
            m.n += c.n;
            m.alive |= c.alive;
            if m.mv.is_none() {
                m.mv = c.mv.clone();
            }
        }
    }
    for m in &mut out {
        m.diff_se = if m.n > 0 { m.diff_se.sqrt() / m.n as f64 } else { f64::INFINITY };
    }
    out.sort_by(|a, b| b.alive.cmp(&a.alive).then(a.mean.total_cmp(&b.mean)));
    Report {
        iterations: reports.iter().map(|r| r.iterations).sum(),
        candidates: out,
        positions: reports.iter().map(|r| r.positions).max().unwrap_or(0),
        screening: reports.iter().all(|r| r.screening),
        truncated: reports.iter().any(|r| r.truncated),
        deep: reports.iter().any(|r| r.deep),
        model: reports.iter().any(|r| r.model),
    }
}
