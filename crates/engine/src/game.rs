use crate::meld::*;
use crate::pairs::{pair_plan, pair_stats};
use crate::policy::{LayStyle, OpenStyle, Policy};
use crate::rules::{Rules, TakenJokerPenalty};
use crate::stakes::Stakes;
use crate::table::{JokerSlot, Side, Table, TableMeld};
use crate::tile::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Opened {
    No,
    Melds,
    Pairs,
}

#[derive(Clone, Debug)]
pub struct Player {
    pub hand: Counts,
    pub opened: Opened,
    pub gosterge_bonus: bool,
    pub penalty: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishInfo {
    pub player: u8,
    pub okey: bool,
    pub elden: bool,
    pub pairs: bool,
    pub last: Tile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Finished(FinishInfo),
    DeckOut,
}

/// What a player does between drawing and discarding.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum OpenChoice {
    /// Do not open (only legal when nothing was taken from the side).
    Stay,
    OpenMelds(OpenStyle, LayStyle),
    OpenPairs(LayStyle),
    /// Already opened.
    Lay(LayStyle),
    /// Finish this turn (elden if not yet opened).
    Finish,
}

impl OpenChoice {
    pub const OPEN: OpenChoice = OpenChoice::OpenMelds(OpenStyle::MaxValue, LayStyle::All);
    pub const PAIRS: OpenChoice = OpenChoice::OpenPairs(LayStyle::All);
    pub const LAY: OpenChoice = OpenChoice::Lay(LayStyle::All);
}

/// One physical action of a turn, recorded when `Game::log` is on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Step {
    Meld {
        shape: Shape,
        owner: u8,
    },
    /// `stands_for` is the tile an okey replaces (the okey itself when a group leaves it open).
    Layoff {
        tile: Tile,
        owner: u8,
        onto: Vec<Tile>,
        side: Side,
        stands_for: Tile,
    },
    Swap {
        owner: u8,
        onto: Vec<Tile>,
        give: Vec<Tile>,
        jokers: u8,
    },
    Discard {
        tile: Tile,
        finish: bool,
    },
}

#[derive(Clone, Debug)]
pub struct Game {
    pub rules: Rules,
    pub gosterge: Tile,
    pub players: [Player; 4],
    pub deck: Vec<Tile>,
    pub table: Table,
    pub top: Option<Tile>,
    pub current: u8,
    pub meld_req: i32,
    pub pair_req: u8,
    pub seen: Counts,
    /// Tiles each player has discarded that nobody took.
    pub piles: [Counts; 4],
    pub last: [Option<Tile>; 4],
    /// Who drew the deck's last tile.
    pub last_draw: Option<u8>,
    pub log: Option<Vec<Step>>,
    pub policy: [Policy; 4],
    /// The match situation seat 0 is playing for, when known.
    pub stakes: Option<Stakes>,
}

const MAX_TURNS: usize = 400;
const LOOKAHEAD_DISCARDS: usize = 3;

pub fn partner(p: u8) -> u8 {
    (p + 2) % 4
}

fn conn(hand: &Counts, t: Tile) -> i32 {
    if t == JOKER {
        return 1000;
    }
    let c = color(t);
    let n = num(t) as i32;
    let mut s = 0;
    for d in [1, 2] {
        for nn in [n - d, n + d] {
            if (1..=13).contains(&nn) {
                s += hand[id(c, nn as u8) as usize] as i32 * if d == 1 { 4 } else { 2 };
            }
        }
    }
    for c2 in 0..NCOLORS {
        if c2 != c {
            s += 3 * hand[id(c2, n as u8) as usize].min(1) as i32;
        }
    }
    s
}

fn remove_tiles(hand: &mut Counts, tiles: &[Tile]) {
    for &t in tiles {
        hand[t as usize] -= 1;
    }
}

impl Game {
    pub fn same_team(&self, a: u8, b: u8) -> bool {
        a == b || (self.rules.partners && a == partner(b))
    }

    pub fn hand_len(&self, p: u8) -> u32 {
        total(&self.players[p as usize].hand)
    }

    fn gosterge_for(&self, p: u8) -> Option<Tile> {
        self.players[p as usize].gosterge_bonus.then_some(self.gosterge)
    }

    fn lay_params(&self, p: u8, must: Option<Tile>) -> (Params, bool) {
        let r = &self.rules;
        let opened = self.players[p as usize].opened;
        let (allow_melds, pair_cap, can_layoff) = match opened {
            Opened::Pairs => (r.pair_opener_can_meld, self.table.pair_room(p, r), r.pair_opener_can_layoff),
            _ => {
                let room = if r.pairs_onto_pair_opener {
                    self.table.pair_owners().into_iter().map(|o| self.table.pair_room(o, r)).sum()
                } else {
                    0
                };
                (r.new_melds_after_open, room, true)
            }
        };
        let params = Params {
            mode: Mode::Penalty,
            cover_w: 1000,
            must,
            allow_melds,
            pair_dump: pair_cap > 0,
            pair_cap,
            joker_runs: self.policy[p as usize].joker_in_runs,
        };
        (params, can_layoff)
    }

    fn lay_search(&self, sv: &mut Solver, p: u8, hand: &Counts, must: Option<Tile>) -> Option<Plan> {
        let (params, can_layoff) = self.lay_params(p, must);
        let attach = can_layoff.then(|| {
            let mut a = self.table.attach(&self.rules);
            if !self.policy[p as usize].joker_in_runs {
                a.tile[JOKER as usize] = self.table.group_has_room();
            }
            a
        });
        sv.run(hand, params, attach.as_ref())
    }

    pub fn open_search(&self, sv: &mut Solver, hand: &Counts, must: Option<Tile>) -> Option<Plan> {
        let params = Params {
            mode: Mode::Open,
            cover_w: 0,
            must,
            allow_melds: true,
            pair_dump: false,
            pair_cap: 0,
            joker_runs: true,
        };
        let plan = sv.run(hand, params, None);
        if must.is_none() {
            sv.note_value(hand, plan.as_ref().map_or(0, |p| p.open_value(&self.rules)));
        }
        plan
    }

    /// Opening melds/pairs go to `p` uncounted; later pairs use up a pair area's per-turn room.
    fn put_melds(&mut self, p: u8, melds: &[Shape], opening: bool) {
        for &s in melds {
            let mut owner = p;
            if s.is_pair() && !opening {
                let own = self.players[p as usize].opened == Opened::Pairs;
                let target = if own {
                    (self.table.pair_room(p, &self.rules) > 0).then_some(p)
                } else {
                    self.table
                        .pair_owners()
                        .into_iter()
                        .find(|&o| self.table.pair_room(o, &self.rules) > 0)
                };
                let Some(o) = target else { continue };
                self.table.pairs_added[o as usize] += 1;
                owner = o;
            }
            remove_tiles(&mut self.players[p as usize].hand, &s.tiles());
            for t in s.tiles() {
                self.seen[t as usize] += 1;
            }
            self.table.melds.push(TableMeld::laid(owner, s));
            if let Some(log) = &mut self.log {
                log.push(Step::Meld { shape: s, owner });
            }
        }
    }

    fn put_layoff(&mut self, p: u8, t: Tile) -> bool {
        let before = self.log.is_some().then(|| self.table.melds.clone());
        let okey_on_runs = self.policy[p as usize].joker_in_runs;
        let Some((i, side)) = self.table.lay_off(t, &self.rules, okey_on_runs) else {
            return false;
        };
        self.players[p as usize].hand[t as usize] -= 1;
        self.seen[t as usize] += 1;
        if let (Some(log), Some(before)) = (&mut self.log, before) {
            let stands_for = match (before[i].shape, side) {
                (Shape::Run { color, lo, .. }, Side::Low) => pos_tile(color, lo - 1),
                (Shape::Run { color, hi, .. }, Side::High) => pos_tile(color, hi + 1),
                _ => t,
            };
            log.push(Step::Layoff {
                tile: t,
                owner: before[i].owner,
                onto: before[i].shape.tiles(),
                side,
                stands_for,
            });
        }
        true
    }

    pub(crate) fn charge_big_pairs(&mut self, p: u8, pairs: u8) {
        let r = &self.rules;
        if r.big_pairs_penalty == 0 || pairs < r.big_pairs_min {
            return;
        }
        self.players[p as usize].penalty -= r.big_pairs_penalty;
    }

    fn charge_take(&mut self, p: u8, t: Tile) {
        let r = &self.rules;
        if !r.take_penalty_enabled {
            return;
        }
        let v = if t == JOKER {
            match r.take_joker_penalty {
                TakenJokerPenalty::None => 0,
                TakenJokerPenalty::PlacedValue => 7,
            }
        } else {
            num(t) as i32
        };
        self.players[p as usize].penalty += v * r.take_penalty_mult;
    }

    /// An opening arrangement in the given style, already checked against the threshold.
    fn open_plan(&self, sv: &mut Solver, p: u8, must: Option<Tile>, style: OpenStyle) -> Option<Plan> {
        let hand = self.players[p as usize].hand;
        if open_bound(&hand, &self.rules) < self.meld_req || sv.value_at_most(&hand) < self.meld_req {
            return None;
        }
        let base = Params {
            mode: Mode::Open,
            cover_w: 0,
            must,
            allow_melds: true,
            pair_dump: false,
            pair_cap: 0,
            joker_runs: true,
        };
        let mut plan = match style {
            OpenStyle::MaxValue => sv.run(&hand, base, None)?,
            OpenStyle::MaxCover => sv.run(
                &hand,
                Params {
                    mode: Mode::Penalty,
                    cover_w: 1000,
                    ..base
                },
                None,
            )?,
            OpenStyle::SafeOkey => sv.run(&hand, Params { joker_runs: false, ..base }, None)?,
            OpenStyle::NoOkey => {
                if must == Some(JOKER) || hand[JOKER as usize] == 0 {
                    return None;
                }
                let mut h = hand;
                h[JOKER as usize] = 0;
                sv.run(&h, base, None)?
            }
            OpenStyle::Minimal => {
                let full = sv.run(&hand, base, None)?;
                let mut melds = full.melds;
                melds.sort_by(|a, b| {
                    let key = |m: &Shape| (must.is_some_and(|t| m.uses(t)), m.open_value(&self.rules) * 100 / m.len() as i32);
                    key(b).cmp(&key(a))
                });
                let mut value = 0;
                let mut keep = Vec::new();
                for m in melds {
                    if value >= self.meld_req {
                        break;
                    }
                    value += m.open_value(&self.rules);
                    keep.push(m);
                }
                Plan {
                    melds: keep,
                    ..Default::default()
                }
            }
        };
        plan.layoffs.clear();
        if !self.hold_back(&mut plan, must, total(&hand) as usize) {
            return None;
        }
        (plan.open_value(&self.rules) >= self.meld_req).then_some(plan)
    }

    pub fn try_open_melds(&mut self, sv: &mut Solver, p: u8, must: Option<Tile>) -> bool {
        let style = self.policy[p as usize].open_style;
        self.try_open_melds_style(sv, p, must, style)
    }

    pub fn try_open_melds_style(&mut self, sv: &mut Solver, p: u8, must: Option<Tile>, style: OpenStyle) -> bool {
        let Some(plan) = self.open_plan(sv, p, must, style) else {
            return false;
        };
        let v = plan.open_value(&self.rules);
        self.put_melds(p, &plan.melds, true);
        self.players[p as usize].opened = Opened::Melds;
        if self.rules.katlamali {
            self.meld_req = self.meld_req.max(v + self.rules.raise_meld_by);
        }
        true
    }

    pub fn pairs_ok(&self, p: u8, must: Option<Tile>) -> Option<Vec<Shape>> {
        self.pairs_ok_with(p, &self.players[p as usize].hand, must)
    }

    fn pairs_ok_with(&self, p: u8, hand: &Counts, must: Option<Tile>) -> Option<Vec<Shape>> {
        if !self.rules.pairs_enabled || self.players[p as usize].opened != Opened::No {
            return None;
        }
        if pair_stats(hand, &self.rules, self.gosterge_for(p)).0 < self.pair_req as usize {
            return None;
        }
        let pp = pair_plan(hand, &self.rules, self.gosterge_for(p));
        if (pp.pairs.len() as u8) < self.pair_req {
            return None;
        }
        if let Some(m) = must
            && !pp.pairs.iter().any(|s| s.uses(m))
        {
            return None;
        }
        Some(pp.pairs)
    }

    pub fn try_open_pairs(&mut self, p: u8, must: Option<Tile>) -> bool {
        let Some(pairs) = self.pairs_ok(p, must) else { return false };
        let n = pairs.len() as u8;
        self.put_melds(p, &pairs, true);
        self.players[p as usize].opened = Opened::Pairs;
        self.charge_big_pairs(p, n);
        if self.rules.katlamali {
            self.pair_req = self.pair_req.max(n + self.rules.raise_pair_by);
        }
        true
    }

    fn swap_jokers(&mut self, p: u8) {
        if !self.rules.joker_retrieve || !self.table.melds.iter().any(|m| m.shape.jokers() > 0) {
            return;
        }
        loop {
            let hand = &self.players[p as usize].hand;
            let slot = self
                .table
                .joker_slots(&self.rules)
                .into_iter()
                .find(|s| s.tiles.iter().all(|&t| hand[t as usize] > 0));
            let Some(s) = slot else { break };
            self.do_swap(p, &s);
        }
    }

    /// Trade natural tiles for the okey(s) of a table meld. Losing an okey to an opponent
    /// costs the meld's owner a penalty; within a team it is free.
    pub fn do_swap(&mut self, p: u8, s: &JokerSlot) {
        let owner = self.table.melds[s.meld].owner;
        if let Some(log) = &mut self.log {
            let onto = self.table.melds[s.meld].shape.tiles();
            log.push(Step::Swap {
                owner,
                onto,
                give: s.tiles.clone(),
                jokers: s.jokers,
            });
        }
        if !self.same_team(owner, p) {
            self.players[owner as usize].penalty += self.rules.okey_lost_penalty;
        }
        let h = &mut self.players[p as usize].hand;
        for &t in &s.tiles {
            h[t as usize] -= 1;
            self.seen[t as usize] += 1;
        }
        h[JOKER as usize] += s.jokers;
        self.seen[JOKER as usize] -= s.jokers;
        self.table.swap_joker(s);
    }

    pub(crate) fn finish(&mut self, p: u8, last: Tile, elden: bool) -> Outcome {
        self.players[p as usize].hand[last as usize] -= 1;
        self.seen[last as usize] += 1;
        if let Some(log) = &mut self.log {
            log.push(Step::Discard { tile: last, finish: true });
        }
        Outcome::Finished(FinishInfo {
            player: p,
            okey: last == JOKER,
            elden,
            pairs: self.players[p as usize].opened == Opened::Pairs,
            last,
        })
    }

    /// Lay everything an opened player can. Returns Err if the side-taken tile cannot be used.
    pub fn lay(&mut self, sv: &mut Solver, p: u8, must: Option<Tile>, elden: bool) -> Result<Option<Outcome>, ()> {
        self.swap_jokers(p);
        let hand = self.players[p as usize].hand;
        if hand[JOKER as usize] > 0 && must != Some(JOKER) {
            let mut h2 = hand;
            h2[JOKER as usize] -= 1;
            if let Some(plan) = self.lay_search(sv, p, &h2, must)
                && plan.rest.is_empty()
            {
                let backup = self.clone();
                if self.apply_plan_all(p, &plan) {
                    return Ok(Some(self.finish(p, JOKER, elden)));
                }
                *self = backup;
            }
        }
        let Some(mut plan) = self.lay_search(sv, p, &hand, must) else {
            return Err(());
        };
        if !self.hold_back(&mut plan, must, total(&hand) as usize) {
            return Err(());
        }
        self.put_melds(p, &plan.melds, false);
        for &t in &plan.layoffs {
            self.put_layoff(p, t);
        }
        loop {
            if self.hand_len(p) <= 1 {
                break;
            }
            let h = self.players[p as usize].hand;
            let can_layoff = self.lay_params(p, None).1;
            if !can_layoff {
                break;
            }
            let mut any = false;
            for t in 0..NIDS as Tile {
                if h[t as usize] > 0 && self.hand_len(p) > 1 && self.put_layoff(p, t) {
                    any = true;
                }
            }
            if !any {
                break;
            }
        }
        if self.hand_len(p) == 1 {
            let last = tiles_of(&self.players[p as usize].hand)[0];
            return Ok(Some(self.finish(p, last, elden)));
        }
        Ok(None)
    }

    /// A player must keep a tile to discard: if `plan` would lay every tile, keep one back.
    fn hold_back(&self, plan: &mut Plan, must: Option<Tile>, hand_total: usize) -> bool {
        if plan.used() < hand_total {
            return true;
        }
        if let Some(i) = plan.layoffs.iter().position(|&t| t == JOKER) {
            plan.layoffs.remove(i);
            return true;
        }
        if let Some(i) = plan.layoffs.iter().rposition(|&t| Some(t) != must) {
            plan.layoffs.remove(i);
            return true;
        }
        let min = self.rules.meld_min_len;
        if let Some(i) = plan.melds.iter().position(|m| !m.is_pair() && m.len() > min) {
            plan.melds[i] = shrink(plan.melds[i]);
            return true;
        }
        if let Some(i) = plan.melds.iter().rposition(|m| must.is_none_or(|t| !m.uses(t))) {
            plan.melds.remove(i);
            return true;
        }
        false
    }

    fn apply_plan_all(&mut self, p: u8, plan: &Plan) -> bool {
        self.put_melds(p, &plan.melds, false);
        for &t in &plan.layoffs {
            if !self.put_layoff(p, t) {
                return false;
            }
        }
        true
    }

    /// Elden finish: every tile but the discard goes down as the player's own melds (or pairs).
    /// No lay-offs and no okey swaps: those need a normal opening first.
    pub(crate) fn elden_plan(&self, sv: &mut Solver, p: u8, must: Option<Tile>) -> Option<(Vec<Shape>, Tile, bool)> {
        self.elden_plan_with(sv, p, self.players[p as usize].hand, must)
    }

    fn elden_plan_with(&self, sv: &mut Solver, p: u8, hand: Counts, must: Option<Tile>) -> Option<(Vec<Shape>, Tile, bool)> {
        let params = Params {
            mode: Mode::Penalty,
            cover_w: 1000,
            must,
            allow_melds: true,
            pair_dump: false,
            pair_cap: 0,
            joker_runs: true,
        };
        let value_ok = |plan: &Plan| !self.rules.elden_requires_open_min || plan.open_value(&self.rules) >= self.meld_req;
        let without = |sv: &mut Solver, last: Tile| -> Option<Vec<Shape>> {
            if must == Some(last) && hand[last as usize] == 1 {
                return None;
            }
            let mut h = hand;
            h[last as usize] -= 1;
            sv.run(&h, params, None)
                .filter(|pl| pl.rest.is_empty() && value_ok(pl))
                .map(|pl| pl.melds)
        };
        // Two tiles that fit no meld at all rule elden out before any search.
        let hopeless = stranded(&hand, &self.rules, 2) >= 2 || sv.left_over_at_least(&hand) >= 2;
        if let Some(full) = (!hopeless).then(|| sv.run(&hand, params, None)).flatten() {
            if must.is_none() {
                sv.note_left_over(&hand, full.rest.len());
            }
            // Dropping one tile removes at most one leftover, so two leftovers rule elden out.
            if full.rest.len() <= 1 {
                if hand[JOKER as usize] > 0
                    && let Some(melds) = without(sv, JOKER)
                {
                    return Some((melds, JOKER, false));
                }
                let lasts: Vec<Tile> = if full.rest.len() == 1 {
                    full.rest.clone()
                } else {
                    (0..NNAT as Tile).filter(|&t| hand[t as usize] > 0).collect()
                };
                for last in lasts {
                    if let Some(melds) = without(sv, last) {
                        return Some((melds, last, false));
                    }
                }
            }
        }
        if self.rules.pairs_enabled && pair_stats(&hand, &self.rules, self.gosterge_for(p)).1 == 1 {
            let pp = pair_plan(&hand, &self.rules, self.gosterge_for(p));
            if pp.rest.len() == 1 && must.is_none_or(|m| pp.pairs.iter().any(|s| s.uses(m))) {
                return Some((pp.pairs, pp.rest[0], true));
            }
        }
        None
    }

    pub fn elden_possible(&self, sv: &mut Solver, p: u8, must: Option<Tile>) -> bool {
        self.elden_plan(sv, p, must).is_some()
    }

    fn do_elden(&mut self, sv: &mut Solver, p: u8, must: Option<Tile>) -> Option<Outcome> {
        let (melds, last, pairs) = self.elden_plan(sv, p, must)?;
        self.put_melds(p, &melds, true);
        self.players[p as usize].opened = if pairs { Opened::Pairs } else { Opened::Melds };
        Some(self.finish(p, last, true))
    }

    /// Apply an open/lay choice for player `p` who has just drawn. Ok(Some) means the hand ended.
    pub fn apply_choice(&mut self, sv: &mut Solver, p: u8, choice: OpenChoice, taken: Option<Tile>) -> Result<Option<Outcome>, ()> {
        let was_unopened = self.players[p as usize].opened == Opened::No;
        match choice {
            OpenChoice::Stay => {
                if taken.is_some() && self.rules.take_must_use {
                    return Err(());
                }
                Ok(None)
            }
            OpenChoice::Finish => {
                if was_unopened {
                    self.do_elden(sv, p, taken).map(Some).ok_or(())
                } else {
                    match self.lay(sv, p, taken, false)? {
                        Some(o) => Ok(Some(o)),
                        None => Err(()),
                    }
                }
            }
            OpenChoice::OpenMelds(style, lay) => {
                if !was_unopened || !self.try_open_melds_style(sv, p, taken, style) {
                    return Err(());
                }
                self.after_open(sv, p, lay)
            }
            OpenChoice::OpenPairs(lay) => {
                if !self.try_open_pairs(p, taken) {
                    return Err(());
                }
                self.after_open(sv, p, lay)
            }
            OpenChoice::Lay(lay) => {
                if was_unopened {
                    return Err(());
                }
                match lay {
                    LayStyle::All => self.lay(sv, p, taken, false),
                    LayStyle::AllSafe => self.lay_safe(sv, p, taken, false),
                    LayStyle::Nothing => {
                        if taken.is_some() && self.rules.take_must_use {
                            return Err(());
                        }
                        Ok(None)
                    }
                }
            }
        }
    }

    fn after_open(&mut self, sv: &mut Solver, p: u8, lay: LayStyle) -> Result<Option<Outcome>, ()> {
        match lay {
            LayStyle::Nothing => Ok(None),
            LayStyle::All => self.lay(sv, p, None, self.rules.open_finish_is_elden),
            LayStyle::AllSafe => self.lay_safe(sv, p, None, self.rules.open_finish_is_elden),
        }
    }

    /// `lay`, with okeys kept out of runs for this one turn.
    fn lay_safe(&mut self, sv: &mut Solver, p: u8, must: Option<Tile>, elden: bool) -> Result<Option<Outcome>, ()> {
        let saved = self.policy[p as usize].joker_in_runs;
        self.policy[p as usize].joker_in_runs = false;
        let res = self.lay(sv, p, must, elden);
        self.policy[p as usize].joker_in_runs = saved;
        res
    }

    /// Whether the bot keeps a ready opening in hand for now.
    pub fn should_hold(&self, p: u8) -> bool {
        let pol = &self.policy[p as usize];
        let active = (0..4u8).any(|q| q != p && self.players[q as usize].opened != Opened::No);
        self.deck.len() as i32 > if active { pol.hold_deck_active } else { pol.hold_deck_quiet }
    }

    pub fn discard_penalty(&self, p: u8, t: Tile) -> i32 {
        let r = &self.rules;
        if self.hand_len(p) <= 1 {
            return 0;
        }
        if t == JOKER {
            return r.penalty_discard_joker;
        }
        let opened = self.players[p as usize].opened != Opened::No;
        if (opened || r.islek_applies_unopened) && self.table.is_islek(t, r) {
            return r.penalty_islek;
        }
        0
    }

    pub fn discard(&mut self, p: u8, t: Tile) {
        debug_assert!(self.players[p as usize].hand[t as usize] > 0, "discarding a tile not in hand");
        let pen = self.discard_penalty(p, t);
        let pl = &mut self.players[p as usize];
        pl.penalty += pen;
        pl.hand[t as usize] -= 1;
        self.seen[t as usize] += 1;
        self.piles[p as usize][t as usize] += 1;
        self.last[p as usize] = Some(t);
        self.top = Some(t);
        if let Some(log) = &mut self.log {
            log.push(Step::Discard { tile: t, finish: false });
        }
    }

    fn live_copies(&self, p: u8, t: Tile) -> i32 {
        2 - self.seen[t as usize] as i32 - self.players[p as usize].hand[t as usize] as i32
    }

    fn pursue_pairs(&self, sv: &mut Solver, p: u8) -> bool {
        let pl = &self.players[p as usize];
        if !self.rules.pairs_enabled || pl.opened != Opened::No {
            return false;
        }
        if (pair_stats(&pl.hand, &self.rules, self.gosterge_for(p)).0 as i32) < self.pair_req as i32 - 1 {
            return false;
        }
        let v = self.open_search(sv, &pl.hand, None).map_or(0, |pl| pl.open_value(&self.rules));
        v < self.meld_req - 25
    }

    pub fn bot_discard(&self, sv: &mut Solver, p: u8) -> Tile {
        self.discard_scores(sv, p)
            .into_iter()
            .min_by_key(|&(_, s)| s)
            .map_or(JOKER, |(t, _)| t)
    }

    /// Heuristic cost of keeping each distinct tile; the lowest is the bot's discard.
    pub fn discard_scores(&self, sv: &mut Solver, p: u8) -> Vec<(Tile, i32)> {
        let pol = self.policy[p as usize];
        let pl = &self.players[p as usize];
        let hand = pl.hand;
        let pairs_mode = pl.opened == Opened::Pairs || self.pursue_pairs(sv, p);
        let mut in_meld = [0u8; NIDS];
        if pl.opened == Opened::No
            && !pairs_mode
            && let Some(plan) = self.open_search(sv, &hand, None)
        {
            for m in &plan.melds {
                for t in m.tiles() {
                    in_meld[t as usize] += 1;
                }
            }
        }
        let next_unopened = self.players[((p + 1) % 4) as usize].opened == Opened::No;
        let pair_hope = pol.w_pair_keep > 0.0
            && !pairs_mode
            && self.rules.pairs_enabled
            && self.rules.pairs_onto_pair_opener
            && (!self.table.pair_owners().is_empty() || self.players.iter().any(|q| q.opened == Opened::No));
        let mut frees_okey = [false; NIDS];
        if self.rules.joker_retrieve {
            for slot in self.table.joker_slots(&self.rules) {
                if let [t] = slot.tiles[..] {
                    frees_okey[t as usize] = true;
                }
            }
        }
        let unseen = |u: Tile| (2 - self.seen[u as usize] as i32 - hand[u as usize] as i32).max(0) as f32;
        let mut out = Vec::new();
        for t in 0..NIDS as Tile {
            if hand[t as usize] == 0 {
                continue;
            }
            if t == JOKER {
                out.push((t, 50_000 + self.discard_penalty(p, t) * 1000));
                continue;
            }
            let n = num(t) as i32;
            let c = color(t);
            let (mut have, mut live) = (0.0f32, 0.0f32);
            for (d, w) in [(1, pol.w_adj), (2, pol.w_gap)] {
                for nn in [n - d, n + d] {
                    if (1..=13).contains(&nn) {
                        let u = id(c, nn as u8);
                        have += hand[u as usize] as f32 * w;
                        live += unseen(u) * w;
                    }
                }
            }
            for c2 in (0..NCOLORS).filter(|&c2| c2 != c) {
                let u = id(c2, n as u8);
                have += hand[u as usize].min(1) as f32 * pol.w_group;
                live += unseen(u).min(1.0) * pol.w_group;
            }
            let potential = 10.0 * (have + pol.w_live * live);
            let mut keep = if pairs_mode {
                if hand[t as usize] >= 2 {
                    500.0
                } else {
                    (self.live_copies(p, t) * 20 + conn(&hand, t) - if pl.opened == Opened::Pairs { 2 * n } else { 0 }) as f32
                }
            } else if pl.opened == Opened::No {
                let base = if in_meld[t as usize] >= hand[t as usize] {
                    pol.w_meld
                } else {
                    potential
                };
                base + pol.w_value_unopened * n as f32
            } else {
                potential - pol.w_value_opened * n as f32
            };
            if next_unopened {
                keep += pol.w_danger * live * n as f32 / 13.0;
            }
            if frees_okey[t as usize] {
                keep += 2000.0;
            }
            if pair_hope && hand[t as usize] >= 2 {
                keep += pol.w_pair_keep;
            }
            out.push((t, (keep * 10.0) as i32 + self.discard_penalty(p, t) * 1000));
        }
        out
    }

    /// Whether `p` can legally take `t` from the side and use it this turn.
    pub fn can_use_taken(&self, sv: &mut Solver, p: u8, t: Tile) -> bool {
        let mut hand = self.players[p as usize].hand;
        hand[t as usize] += 1;
        match self.players[p as usize].opened {
            Opened::No => {
                if sv.value_at_most(&hand) >= self.meld_req
                    && open_bound(&hand, &self.rules) >= self.meld_req
                    && let Some(plan) = self.open_search(sv, &hand, Some(t))
                    && plan.open_value(&self.rules) >= self.meld_req
                {
                    return true;
                }
                self.pairs_ok_with(p, &hand, Some(t)).is_some() || self.elden_plan_with(sv, p, hand, Some(t)).is_some()
            }
            _ => self.lay_search(sv, p, &hand, Some(t)).is_some(),
        }
    }

    pub fn bot_wants_take(&self, sv: &mut Solver, p: u8) -> bool {
        let Some(t) = self.top else { return false };
        let pl = &self.players[p as usize];
        if t != JOKER && conn(&pl.hand, t) == 0 && pl.hand[t as usize] == 0 {
            let fits = pl.opened != Opened::No && self.table.attach(&self.rules).fits(t);
            if !fits {
                return false;
            }
        }
        if self.rules.take_penalty_enabled && pl.opened != Opened::No {
            return false;
        }
        self.can_use_taken(sv, p, t)
    }

    pub fn draw(&mut self, p: u8) -> Option<Tile> {
        let t = self.deck.pop()?;
        self.players[p as usize].hand[t as usize] += 1;
        if self.deck.is_empty() {
            self.last_draw = Some(p);
        }
        Some(t)
    }

    pub fn take_top(&mut self, p: u8) -> Tile {
        let t = self.top.take().expect("tile to take");
        self.seen[t as usize] -= 1;
        let from = ((p + 3) % 4) as usize;
        self.piles[from][t as usize] = self.piles[from][t as usize].saturating_sub(1);
        self.last[from] = None;
        self.players[p as usize].hand[t as usize] += 1;
        self.charge_take(p, t);
        t
    }

    /// Bot's open/lay decision after drawing.
    pub fn bot_act(&mut self, sv: &mut Solver, p: u8, taken: Option<Tile>) -> Option<Outcome> {
        let opened = self.players[p as usize].opened;
        let pol = self.policy[p as usize];
        let lay = if pol.lay_after_open { LayStyle::All } else { LayStyle::Nothing };
        let unopened = [
            OpenChoice::Finish,
            OpenChoice::OpenMelds(pol.open_style, lay),
            OpenChoice::OpenPairs(lay),
            OpenChoice::Stay,
        ];
        let tries: &[OpenChoice] = match opened {
            Opened::No => &unopened,
            _ => &[OpenChoice::LAY, OpenChoice::Stay],
        };
        let hold = taken.is_none() && opened == Opened::No && self.should_hold(p);
        for &c in tries {
            if c == OpenChoice::Finish && !self.elden_possible(sv, p, taken) {
                continue;
            }
            if hold && matches!(c, OpenChoice::OpenMelds(..) | OpenChoice::OpenPairs(..)) {
                continue;
            }
            if matches!(c, OpenChoice::OpenPairs(..)) && self.pairs_ok(p, taken).is_none() {
                continue;
            }
            // Only laying with a side-taken tile can fail after it has already changed the table.
            let backup = (matches!(c, OpenChoice::Lay(_)) && taken.is_some()).then(|| self.clone());
            match self.apply_choice(sv, p, c, taken) {
                Ok(Some(o)) => return Some(o),
                Ok(None) => {
                    let d = self.bot_discard(sv, p);
                    self.discard(p, d);
                    return None;
                }
                Err(()) => {
                    if let Some(b) = backup {
                        *self = b;
                    }
                }
            }
        }
        let d = self.bot_discard(sv, p);
        self.discard(p, d);
        None
    }

    /// One full bot turn for the current player.
    pub fn bot_turn(&mut self, sv: &mut Solver) -> Option<Outcome> {
        let p = self.current;
        if self.deck.is_empty() {
            return Some(Outcome::DeckOut);
        }
        self.table.new_turn();
        let taken = if self.bot_wants_take(sv, p) {
            Some(self.take_top(p))
        } else {
            if self.draw(p).is_none() {
                return Some(Outcome::DeckOut);
            }
            None
        };
        let res = self.bot_act(sv, p, taken);
        if res.is_none() {
            self.current = (p + 1) % 4;
        }
        res
    }

    /// The opening / laying plans worth trying for a player in the given state.
    pub fn plan_choices(opened: Opened, rich: bool) -> Vec<OpenChoice> {
        match (opened, rich) {
            (Opened::No, false) => vec![OpenChoice::Finish, OpenChoice::OPEN, OpenChoice::PAIRS, OpenChoice::Stay],
            (_, false) => vec![OpenChoice::LAY],
            (Opened::No, true) => {
                let mut v = vec![OpenChoice::Finish];
                for style in [
                    OpenStyle::MaxValue,
                    OpenStyle::MaxCover,
                    OpenStyle::Minimal,
                    OpenStyle::SafeOkey,
                    OpenStyle::NoOkey,
                ] {
                    for lay in [LayStyle::All, LayStyle::AllSafe, LayStyle::Nothing] {
                        v.push(OpenChoice::OpenMelds(style, lay));
                    }
                }
                v.extend([
                    OpenChoice::OpenPairs(LayStyle::All),
                    OpenChoice::OpenPairs(LayStyle::Nothing),
                    OpenChoice::Stay,
                ]);
                v
            }
            (_, true) => vec![
                OpenChoice::Lay(LayStyle::All),
                OpenChoice::Lay(LayStyle::AllSafe),
                OpenChoice::Lay(LayStyle::Nothing),
            ],
        }
    }

    /// Distinct turns for `p` built from those plans: each resulting position with its best few discards.
    /// The game returned with each is the state after the plan, before the discard (None = hand ended).
    pub fn plan_turns(&self, sv: &mut Solver, p: u8, taken: Option<Tile>, rich: bool, discards: usize) -> Vec<(OpenChoice, Option<Tile>)> {
        let mut out = Vec::new();
        let mut seen: Vec<(Counts, Vec<(u8, Shape)>, i32, bool)> = Vec::new();
        for choice in Self::plan_choices(self.players[p as usize].opened, rich) {
            let mut g = self.clone();
            let Ok(res) = g.apply_choice(sv, p, choice, taken) else { continue };
            let key = (
                g.players[p as usize].hand,
                g.table.melds.iter().map(|m| (m.owner, m.shape)).collect::<Vec<_>>(),
                g.players.iter().map(|pl| pl.penalty).sum::<i32>(),
                res.is_some(),
            );
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            match res {
                Some(_) => out.push((choice, None)),
                None => {
                    let mut scores = g.discard_scores(sv, p);
                    scores.sort_by_key(|&(_, s)| s);
                    out.extend(scores.into_iter().take(discards).map(|(t, _)| (choice, Some(t))));
                }
            }
        }
        out
    }

    /// (best opening total, pairs, tiles left loose after laying everything, their points)
    pub fn hand_shape(&self, sv: &mut Solver, p: u8) -> (i32, usize, usize, i32) {
        let pl = &self.players[p as usize];
        let hand = pl.hand;
        let (value, pairs, plan) = match pl.opened {
            Opened::No => {
                let value = self.open_search(sv, &hand, None).map_or(0, |plan| plan.open_value(&self.rules));
                let pairs = pair_stats(&hand, &self.rules, self.gosterge_for(p)).0;
                let cover = Params {
                    mode: Mode::Penalty,
                    cover_w: 1000,
                    must: None,
                    allow_melds: true,
                    pair_dump: false,
                    pair_cap: 0,
                    joker_runs: true,
                };
                (value, pairs, sv.run(&hand, cover, None))
            }
            _ => (0, 0, self.lay_search(sv, p, &hand, None)),
        };
        match plan {
            Some(plan) => (
                value,
                pairs,
                plan.rest.len(),
                plan.rest.iter().map(|&t| penalty_of(t, &self.rules)).sum(),
            ),
            None => (value, pairs, total(&hand) as usize, hand_penalty(&hand, &self.rules)),
        }
    }

    /// Per tile `p` could draw: (lets `p` open or, once open, go out; gets used at all)
    pub fn outs(&self, sv: &mut Solver, p: u8) -> [(bool, bool); NIDS] {
        let pl = &self.players[p as usize];
        let hand = pl.hand;
        let mut out = [(false, false); NIDS];
        let live = |u: usize| self.seen[u] + hand[u] < 2;
        let joins = |u: usize| u == JOKER as usize || hand[u] > 0 || conn(&hand, u as Tile) > 0;
        if pl.opened == Opened::No {
            let value = self.open_search(sv, &hand, None).map_or(0, |plan| plan.open_value(&self.rules));
            let pairs = pair_stats(&hand, &self.rules, self.gosterge_for(p)).0;
            for u in (0..NIDS).filter(|&u| live(u) && joins(u)) {
                let mut h = hand;
                h[u] += 1;
                let v = self.open_search(sv, &h, None).map_or(0, |plan| plan.open_value(&self.rules));
                let n = pair_stats(&h, &self.rules, self.gosterge_for(p)).0;
                out[u] = (v >= self.meld_req || n >= self.pair_req as usize, v > value || n > pairs);
            }
        } else {
            let attach = self.table.attach(&self.rules);
            let loose = self
                .lay_search(sv, p, &hand, None)
                .map_or(total(&hand) as usize, |plan| plan.rest.len());
            for u in (0..NIDS).filter(|&u| live(u) && (joins(u) || attach.tile[u])) {
                let mut h = hand;
                h[u] += 1;
                let left = self.lay_search(sv, p, &h, None).map_or(total(&h) as usize, |plan| plan.rest.len());
                out[u] = (left <= 1, left <= loose);
            }
        }
        out
    }

    /// Score gap from `p`'s side, times three: own score against the opponents' (team against team when partnered).
    pub fn gap3(&self, s: &[i32; 4], p: u8) -> i32 {
        let (me, mate) = (p as usize, partner(p) as usize);
        if self.rules.partners {
            3 * (s[me] + s[mate]) - 3 * (0..4).filter(|&q| q != me && q != mate).map(|q| s[q]).sum::<i32>()
        } else {
            3 * s[me] - (0..4).filter(|&q| q != me).map(|q| s[q]).sum::<i32>()
        }
    }

    /// What `p` is trying to make small, times three: the chance of losing the match (per mille)
    /// when seat 0's match situation is known, otherwise the score gap.
    pub fn cost3(&self, s: &[i32; 4], p: u8) -> i32 {
        match self.stakes {
            // Among lines with the same chance, the better score still decides: the gap rides along
            // as a fraction of one per mille.
            Some(st) if p == 0 => (st.loss(s, self.rules.partners) * 3000.0).round() as i32 * 128 + (self.gap3(s, p) / 64).clamp(-127, 127),
            _ => self.gap3(s, p),
        }
    }

    /// One turn for `hero`, chosen by trying every plan and playing each out to the end of the hand.
    fn lookahead_turn(&mut self, sv: &mut Solver, hero: u8, deeper: u8) -> Option<Outcome> {
        if self.deck.is_empty() {
            return Some(Outcome::DeckOut);
        }
        self.table.new_turn();
        let taken = if self.bot_wants_take(sv, hero) {
            Some(self.take_top(hero))
        } else {
            if self.draw(hero).is_none() {
                return Some(Outcome::DeckOut);
            }
            None
        };
        self.lookahead_act(sv, hero, taken, deeper)
    }

    /// `hero` has drawn: pick the plan and discard whose played-out continuation is best, and make it.
    pub fn lookahead_act(&mut self, sv: &mut Solver, hero: u8, taken: Option<Tile>, deeper: u8) -> Option<Outcome> {
        let mut best: Option<(i32, Game, Option<Outcome>)> = None;
        for (choice, discard) in self.plan_turns(sv, hero, taken, true, LOOKAHEAD_DISCARDS) {
            let mut g = self.clone();
            let Ok(res) = g.apply_choice(sv, hero, choice, taken) else {
                continue;
            };
            if res.is_none() {
                let Some(t) = discard.filter(|&t| g.players[hero as usize].hand[t as usize] > 0) else {
                    continue;
                };
                g.discard(hero, t);
                g.current = (hero + 1) % 4;
            }
            let mut sim = g.clone();
            let end = match res {
                Some(o) => o,
                None if deeper > 0 => sim.play_out_lookahead(sv, hero, deeper),
                None => sim.play_out(sv),
            };
            let gap = sim.cost3(&sim.scores(&end), hero);
            if best.as_ref().is_none_or(|b| gap < b.0) {
                best = Some((gap, g, res));
            }
        }
        match best {
            Some((_, g, res)) => {
                *self = g;
                res
            }
            None => {
                let res = self.bot_act(sv, hero, taken);
                if res.is_none() {
                    self.current = (hero + 1) % 4;
                }
                res
            }
        }
    }

    /// Bots play the hand out, except that `hero` searches its next `depth` turns instead of following the bot.
    /// Inside one simulated deal the hero sees everything, so this flatters lines that wait for a known tile;
    /// only an arena comparison says whether it helps.
    pub fn play_out_lookahead(&mut self, sv: &mut Solver, hero: u8, depth: u8) -> Outcome {
        let mut left = depth;
        for _ in 0..MAX_TURNS {
            let res = if self.current == hero && left > 0 {
                left -= 1;
                self.lookahead_turn(sv, hero, left)
            } else {
                self.bot_turn(sv)
            };
            if let Some(o) = res {
                return o;
            }
        }
        Outcome::DeckOut
    }

    pub fn play_out(&mut self, sv: &mut Solver) -> Outcome {
        for _ in 0..MAX_TURNS {
            if let Some(o) = self.bot_turn(sv) {
                return o;
            }
        }
        Outcome::DeckOut
    }

    pub fn multiplier(&self, f: &FinishInfo) -> i32 {
        let r = &self.rules;
        let mut m = 1;
        if f.okey {
            m *= r.mult_okey_finish;
        }
        if f.elden {
            m *= r.mult_elden;
        }
        if f.pairs {
            m *= r.mult_pair_finish;
        }
        m
    }

    pub fn scores(&self, out: &Outcome) -> [i32; 4] {
        let r = &self.rules;
        let (mult, fin) = match out {
            Outcome::Finished(f) => (self.multiplier(f), Some(f.player)),
            Outcome::DeckOut => (1, None),
        };
        let mut s = [0i32; 4];
        for p in 0..4u8 {
            let pl = &self.players[p as usize];
            let base = if Some(p) == fin {
                r.score_finish
            } else {
                match pl.opened {
                    Opened::No => r.score_unopened + pl.hand[JOKER as usize] as i32 * r.joker_in_hand,
                    Opened::Melds => hand_penalty(&pl.hand, r),
                    Opened::Pairs => hand_penalty(&pl.hand, r) * r.pair_hand_mult,
                }
            };
            s[p as usize] = base * mult + pl.penalty;
        }
        if r.partners
            && let Some(f) = fin
        {
            let m = partner(f);
            s[m as usize] = r.score_finish * mult + self.players[m as usize].penalty;
        }
        s
    }
}

fn shrink(m: Shape) -> Shape {
    match m {
        Shape::Run { color, lo, hi, jokers } => Shape::Run {
            color,
            lo,
            hi: hi - 1,
            jokers: jokers & !(1 << hi),
        },
        Shape::Group { num, colors, jokers } if jokers > 0 => Shape::Group {
            num,
            colors,
            jokers: jokers - 1,
        },
        Shape::Group { num, colors, jokers } => {
            let top = 7 - colors.leading_zeros() as u8;
            Shape::Group {
                num,
                colors: colors & !(1 << top),
                jokers,
            }
        }
        other => other,
    }
}

impl Game {
    /// A fresh random deal; seat `starter` gets the extra tile and moves first.
    pub fn deal(rules: &Rules, rng: &mut crate::rng::Rng, starter: u8) -> Game {
        let mut pool = tiles_of(&full_set());
        rng.shuffle(&mut pool);
        let gosterge = loop {
            let i = rng.below(pool.len());
            if pool[i] != JOKER {
                break pool.swap_remove(i);
            }
        };
        let mut seen = [0u8; NIDS];
        seen[gosterge as usize] += 1;
        let players: Vec<Player> = (0..4u8)
            .map(|p| {
                let n = rules.hand_size + if p == starter { rules.starter_extra } else { 0 };
                let mut hand = [0u8; NIDS];
                for _ in 0..n {
                    hand[pool.pop().unwrap() as usize] += 1;
                }
                let gosterge_bonus = hand[gosterge as usize] > 0;
                Player {
                    hand,
                    opened: Opened::No,
                    gosterge_bonus,
                    penalty: 0,
                }
            })
            .collect();
        Game {
            rules: rules.clone(),
            gosterge,
            players: players.try_into().expect("4 players"),
            deck: pool,
            table: Table::default(),
            top: None,
            current: starter,
            meld_req: rules.open_min,
            pair_req: rules.pair_min,
            seen,
            piles: [[0; NIDS]; 4],
            last: [None; 4],
            last_draw: None,
            log: None,
            policy: [Policy::default(); 4],
            stakes: None,
        }
    }

    /// Starter's first move: no draw, just act and discard.
    pub fn starter_turn(&mut self, sv: &mut Solver) -> Option<Outcome> {
        let p = self.current;
        self.table.new_turn();
        let res = self.bot_act(sv, p, None);
        if res.is_none() {
            self.current = (p + 1) % 4;
        }
        res
    }

    /// The same game with the seats turned so that seat `k` becomes seat 0.
    pub fn rotated(&self, k: u8) -> Game {
        let mut g = self.clone();
        let k = k % 4;
        g.players.rotate_left(k as usize);
        g.policy.rotate_left(k as usize);
        g.piles.rotate_left(k as usize);
        g.last.rotate_left(k as usize);
        g.table.pairs_added.rotate_left(k as usize);
        for m in &mut g.table.melds {
            m.owner = (m.owner + 4 - k) % 4;
        }
        g.current = (g.current + 4 - k) % 4;
        if k != 0 {
            g.stakes = None;
        }
        g
    }

    /// Total tiles accounted for: hands + deck + everything visible.
    pub fn tile_count(&self) -> u32 {
        self.players.iter().map(|p| total(&p.hand)).sum::<u32>() + self.deck.len() as u32 + total(&self.seen)
    }
}
