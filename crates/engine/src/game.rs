use crate::meld::*;
use crate::pairs::{pair_plan, pair_stats};
use crate::policy::{LayStyle, OpenStyle, Policy};
use crate::rules::Rules;
use crate::stakes::Stakes;
use crate::table::{JokerSlot, Side, Table, TableMeld};
use crate::tile::*;
use serde::{Deserialize, Serialize};

mod bot;
mod plans;
use bot::*;

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
    pub has_indicator: bool,
    pub penalty: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishInfo {
    pub player: u8,
    pub okey: bool,
    #[serde(alias = "elden")]
    pub in_one: bool,
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
    /// Finish this turn (in-one if not yet opened).
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
    /// `losers` are the players who laid the okeys and are charged for losing them.
    Swap {
        owner: u8,
        onto: Vec<Tile>,
        give: Vec<Tile>,
        jokers: u8,
        #[serde(default)]
        losers: Vec<u8>,
    },
    Discard {
        tile: Tile,
        finish: bool,
    },
}

#[derive(Clone, Debug)]
pub struct Game {
    pub rules: Rules,
    pub indicator: Tile,
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

pub(crate) const MAX_TURNS: usize = 400;
const LOOKAHEAD_DISCARDS: usize = 3;

pub fn partner(p: u8) -> u8 {
    (p + 2) % 4
}

impl Game {
    pub fn hand_len(&self, p: u8) -> u32 {
        total(&self.players[p as usize].hand)
    }

    fn indicator_for(&self, p: u8) -> Option<Tile> {
        self.players[p as usize].has_indicator.then_some(self.indicator)
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
            allow_melds,
            pair_dump: pair_cap > 0,
            pair_cap,
            joker_runs: self.policy[p as usize].joker_in_runs,
            ..Params::cover(must)
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
        let plan = sv.run(hand, Params::open(must), None);
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
            self.place(p, owner, s);
        }
    }

    /// `p` lays `shape` from hand into `owner`'s melds.
    pub(crate) fn place(&mut self, p: u8, owner: u8, shape: Shape) {
        for t in shape.tiles() {
            self.players[p as usize].hand[t as usize] -= 1;
            self.seen[t as usize] += 1;
        }
        let mut meld = TableMeld::laid(owner, shape);
        for _ in 0..shape.jokers() {
            meld.okey_from(p, Side::Group);
        }
        self.table.melds.push(meld);
        if let Some(log) = &mut self.log {
            log.push(Step::Meld { shape, owner });
        }
    }

    /// `onto` is the meld as it stood before `tile` went to that side of it.
    pub(crate) fn log_layoff(&mut self, onto: &TableMeld, tile: Tile, side: Side) {
        let Some(log) = &mut self.log else { return };
        let stands_for = match (onto.shape, side) {
            (Shape::Run { color, lo, .. }, Side::Low) => pos_tile(color, lo - 1),
            (Shape::Run { color, hi, .. }, Side::High) => pos_tile(color, hi + 1),
            _ => tile,
        };
        log.push(Step::Layoff {
            tile,
            owner: onto.owner,
            onto: onto.shape.tiles(),
            side,
            stands_for,
        });
    }

    fn put_layoff(&mut self, p: u8, t: Tile) -> bool {
        let before = self.log.is_some().then(|| self.table.melds.clone());
        let okey_on_runs = self.policy[p as usize].joker_in_runs;
        let Some((i, side)) = self.table.lay_off(t, &self.rules, okey_on_runs) else {
            return false;
        };
        self.players[p as usize].hand[t as usize] -= 1;
        self.seen[t as usize] += 1;
        if t == JOKER {
            self.table.melds[i].okey_from(p, side);
        }
        if let Some(before) = before {
            self.log_layoff(&before[i], t, side);
        }
        true
    }

    pub(crate) fn charge_big_pairs(&mut self, p: u8, pairs: u8) {
        self.players[p as usize].penalty -= self.rules.big_pairs_reward(pairs);
    }

    /// `p` has opened with `value` points or pairs; at a raising table the next opening needs more.
    pub(crate) fn mark_opened(&mut self, p: u8, kind: Opened, value: i32) {
        self.players[p as usize].opened = kind;
        let (meld, pair) = self.rules.thresholds([(kind, value)]);
        self.meld_req = self.meld_req.max(meld);
        self.pair_req = self.pair_req.max(pair);
    }

    /// An opening arrangement in the given style, already checked against the threshold.
    /// A side-taken tile has to be used, so with one any arrangement that opens will do.
    fn open_plan(&self, sv: &mut Solver, p: u8, must: Option<Tile>, style: OpenStyle) -> Option<Plan> {
        self.styled_plan(sv, p, must, style).or_else(|| {
            (must.is_some() && style != OpenStyle::MaxValue)
                .then(|| self.styled_plan(sv, p, must, OpenStyle::MaxValue))
                .flatten()
        })
    }

    fn styled_plan(&self, sv: &mut Solver, p: u8, must: Option<Tile>, style: OpenStyle) -> Option<Plan> {
        let hand = self.players[p as usize].hand;
        if open_bound(&hand, &self.rules) < self.meld_req || sv.value_at_most(&hand) < self.meld_req {
            return None;
        }
        let base = Params::open(must);
        let mut plan = match style {
            OpenStyle::MaxValue => sv.run(&hand, base, None)?,
            OpenStyle::MaxCover => sv.run(&hand, Params::cover(must), None)?,
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
        self.mark_opened(p, Opened::Melds, v);
        true
    }

    pub fn pairs_ok(&self, p: u8, must: Option<Tile>) -> Option<Vec<Shape>> {
        self.pairs_ok_with(p, &self.players[p as usize].hand, must)
    }

    fn pairs_ok_with(&self, p: u8, hand: &Counts, must: Option<Tile>) -> Option<Vec<Shape>> {
        if !self.rules.pairs_enabled || self.players[p as usize].opened != Opened::No {
            return None;
        }
        if pair_stats(hand, &self.rules, self.indicator_for(p)).0 < self.pair_req as usize {
            return None;
        }
        let pp = pair_plan(hand, &self.rules, self.indicator_for(p));
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
        self.mark_opened(p, Opened::Pairs, n as i32);
        self.charge_big_pairs(p, n);
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

    /// Trade natural tiles for the okey(s) of a table meld. Whoever laid an okey pays for losing it
    /// to an opponent; taking back one's own or a partner's is free.
    pub fn do_swap(&mut self, p: u8, s: &JokerSlot) {
        let owner = self.table.melds[s.meld].owner;
        let onto = self.log.is_some().then(|| self.table.melds[s.meld].shape.tiles());
        let h = &mut self.players[p as usize].hand;
        for &t in &s.tiles {
            h[t as usize] -= 1;
            self.seen[t as usize] += 1;
        }
        h[JOKER as usize] += s.jokers;
        self.seen[JOKER as usize] -= s.jokers;
        let losers: Vec<u8> = self
            .table
            .swap_joker(s, p)
            .into_iter()
            .flatten()
            .filter(|&l| !self.rules.same_team(l, p))
            .collect();
        for &l in &losers {
            self.players[l as usize].penalty += self.rules.okey_lost_penalty;
        }
        if let (Some(log), Some(onto)) = (&mut self.log, onto) {
            log.push(Step::Swap {
                owner,
                onto,
                give: s.tiles.clone(),
                jokers: s.jokers,
                losers,
            });
        }
    }

    pub(crate) fn finish(&mut self, p: u8, last: Tile, in_one: bool) -> Outcome {
        self.players[p as usize].hand[last as usize] -= 1;
        self.seen[last as usize] += 1;
        if let Some(log) = &mut self.log {
            log.push(Step::Discard { tile: last, finish: true });
        }
        Outcome::Finished(FinishInfo {
            player: p,
            okey: last == JOKER,
            in_one,
            pairs: self.players[p as usize].opened == Opened::Pairs,
            last,
        })
    }

    /// Lay everything an opened player can. Returns Err if the side-taken tile cannot be used.
    pub fn lay(&mut self, sv: &mut Solver, p: u8, must: Option<Tile>, in_one: bool) -> Result<Option<Outcome>, ()> {
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
                    return Ok(Some(self.finish(p, JOKER, in_one)));
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
            return Ok(Some(self.finish(p, last, in_one)));
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

    /// In-one finish: every tile but the discard goes down as the player's own melds (or pairs).
    /// No lay-offs and no okey swaps: those need a normal opening first.
    pub(crate) fn in_one_plan(&self, sv: &mut Solver, p: u8, must: Option<Tile>) -> Option<(Vec<Shape>, Tile, bool)> {
        self.in_one_plan_with(sv, p, self.players[p as usize].hand, must)
    }

    fn in_one_plan_with(&self, sv: &mut Solver, p: u8, hand: Counts, must: Option<Tile>) -> Option<(Vec<Shape>, Tile, bool)> {
        let params = Params::cover(must);
        let value_ok = |plan: &Plan| !self.rules.in_one_requires_open_min || plan.open_value(&self.rules) >= self.meld_req;
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
        // Two tiles that fit no meld at all rule in-one out before any search.
        let hopeless = stranded(&hand, &self.rules, 2) >= 2 || sv.left_over_at_least(&hand) >= 2;
        if let Some(full) = (!hopeless).then(|| sv.run(&hand, params, None)).flatten() {
            if must.is_none() {
                sv.note_left_over(&hand, full.rest.len());
            }
            // Dropping one tile removes at most one leftover, so two leftovers rule in-one out.
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
        if self.rules.pairs_enabled && pair_stats(&hand, &self.rules, self.indicator_for(p)).1 == 1 {
            let pp = pair_plan(&hand, &self.rules, self.indicator_for(p));
            if pp.rest.len() == 1 && must.is_none_or(|m| pp.pairs.iter().any(|s| s.uses(m))) {
                return Some((pp.pairs, pp.rest[0], true));
            }
        }
        None
    }

    pub fn in_one_possible(&self, sv: &mut Solver, p: u8, must: Option<Tile>) -> bool {
        self.in_one_plan(sv, p, must).is_some()
    }

    fn do_in_one(&mut self, sv: &mut Solver, p: u8, must: Option<Tile>) -> Option<Outcome> {
        let (melds, last, pairs) = self.in_one_plan(sv, p, must)?;
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
                    self.do_in_one(sv, p, taken).map(Some).ok_or(())
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
            LayStyle::All => self.lay(sv, p, None, self.rules.open_finish_is_in_one),
            LayStyle::AllSafe => self.lay_safe(sv, p, None, self.rules.open_finish_is_in_one),
        }
    }

    /// `lay`, with okeys kept out of runs for this one turn.
    fn lay_safe(&mut self, sv: &mut Solver, p: u8, must: Option<Tile>, in_one: bool) -> Result<Option<Outcome>, ()> {
        let saved = self.policy[p as usize].joker_in_runs;
        self.policy[p as usize].joker_in_runs = false;
        let res = self.lay(sv, p, must, in_one);
        self.policy[p as usize].joker_in_runs = saved;
        res
    }

    pub fn discard_penalty(&self, p: u8, t: Tile) -> i32 {
        if self.hand_len(p) <= 1 {
            return 0;
        }
        let opened = self.players[p as usize].opened != Opened::No;
        self.table.discard_penalty(t, opened, &self.rules)
    }

    /// Throw `want` when it is in hand, otherwise what the bot would throw.
    pub fn discard_or_bot(&mut self, sv: &mut Solver, p: u8, want: Option<Tile>) {
        let t = want
            .filter(|&t| self.players[p as usize].hand[t as usize] > 0)
            .unwrap_or_else(|| self.bot_discard(sv, p));
        self.discard(p, t);
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
                self.pairs_ok_with(p, &hand, Some(t)).is_some() || self.in_one_plan_with(sv, p, hand, Some(t)).is_some()
            }
            _ => {
                let all = total(&hand) as usize;
                let finish_on_okey = hand[JOKER as usize] > 0 && t != JOKER && {
                    let mut rest = hand;
                    rest[JOKER as usize] -= 1;
                    self.lay_search(sv, p, &rest, Some(t)).is_some_and(|plan| plan.rest.is_empty())
                };
                finish_on_okey
                    || self
                        .lay_search(sv, p, &hand, Some(t))
                        .is_some_and(|mut plan| self.hold_back(&mut plan, Some(t), all))
            }
        }
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
        self.players[p as usize].penalty += self.rules.take_cost(t);
        t
    }

    pub fn scores(&self, out: &Outcome) -> [i32; 4] {
        let r = &self.rules;
        let (mult, fin) = match out {
            Outcome::Finished(f) => (r.finish_mult(f.okey, f.in_one, f.pairs), Some(f.player)),
            Outcome::DeckOut => (1, None),
        };
        let mut s = [0i32; 4];
        for p in 0..4u8 {
            let pl = &self.players[p as usize];
            let base = if fin.is_some_and(|f| r.same_team(f, p)) {
                r.score_finish
            } else {
                r.hand_charge(pl.opened, hand_penalty(&pl.hand, r), pl.hand[JOKER as usize] as i32)
            };
            s[p as usize] = base * mult + pl.penalty;
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
        let indicator = loop {
            let i = rng.below(pool.len());
            if pool[i] != JOKER {
                break pool.swap_remove(i);
            }
        };
        let mut seen = [0u8; NIDS];
        seen[indicator as usize] += 1;
        let players: Vec<Player> = (0..4u8)
            .map(|p| {
                let n = rules.hand_size + if p == starter { rules.starter_extra } else { 0 };
                let mut hand = [0u8; NIDS];
                for _ in 0..n {
                    hand[pool.pop().unwrap() as usize] += 1;
                }
                let has_indicator = hand[indicator as usize] > 0;
                Player {
                    hand,
                    opened: Opened::No,
                    has_indicator,
                    penalty: 0,
                }
            })
            .collect();
        Game {
            rules: rules.clone(),
            indicator,
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
        g.last_draw = g.last_draw.map(|p| (p + 4 - k) % 4);
        for l in g.table.melds.iter_mut().flat_map(|m| m.lent.iter_mut()).flatten() {
            l.0 = (l.0 + 4 - k) % 4;
        }
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
