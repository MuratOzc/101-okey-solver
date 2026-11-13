//! Every legal thing a player can do in one turn, as explicit steps.

use crate::game::{Game, Opened, Outcome, Step};
use crate::meld::{Shape, Solver, pos_tile};
use crate::pairs::pair_plan;
use crate::rules::Rules;
use crate::table::{JokerSlot, Side, TableMeld};
use crate::tile::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Act {
    /// A new run, group or pair of the player's own.
    Meld(Shape),
    /// Put natural tiles into a table meld and take its okey(s).
    Swap(JokerSlot),
    Layoff {
        meld: usize,
        tile: Tile,
        side: Side,
    },
    /// A pair laid into another player's pair area.
    Gift {
        owner: u8,
        shape: Shape,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Move {
    pub acts: Vec<Act>,
    pub discard: Tile,
}

/// What `play_acts` found out about the turn so far.
pub struct ActsDone {
    /// Melds were laid but the opening threshold was not reached: legal only as an elden finish.
    pub short_opening: bool,
    pub only_own_melds: bool,
    pub pairs: bool,
}

impl Game {
    fn holds(&self, p: u8, tiles: &[Tile]) -> bool {
        let mut h = self.players[p as usize].hand;
        tiles.iter().all(|&t| {
            let ok = h[t as usize] > 0;
            if ok {
                h[t as usize] -= 1;
            }
            ok
        })
    }

    fn place(&mut self, p: u8, owner: u8, shape: Shape) {
        for t in shape.tiles() {
            self.players[p as usize].hand[t as usize] -= 1;
            self.seen[t as usize] += 1;
        }
        self.table.melds.push(TableMeld::laid(owner, shape));
        if let Some(log) = &mut self.log {
            log.push(Step::Meld { shape, owner });
        }
    }

    /// Mark the player opened once this turn's melds reach the threshold.
    fn settle_opening(&mut self, p: u8, value: i32, pairs: u8) -> bool {
        let r = &self.rules;
        if value > 0 && value >= self.meld_req {
            self.players[p as usize].opened = Opened::Melds;
            if r.katlamali {
                self.meld_req = self.meld_req.max(value + r.raise_meld_by);
            }
            true
        } else if pairs > 0 && pairs >= self.pair_req {
            self.players[p as usize].opened = Opened::Pairs;
            if r.katlamali {
                self.pair_req = self.pair_req.max(pairs + r.raise_pair_by);
            }
            true
        } else {
            false
        }
    }

    /// Carry out the steps of a turn in order, enforcing every table rule. Nothing but the
    /// player's own melds may happen before the opening threshold is on the table.
    pub fn play_acts(&mut self, p: u8, acts: &[Act], taken: Option<Tile>) -> Result<ActsDone, ()> {
        let r = self.rules.clone();
        let pi = p as usize;
        let start = self.players[pi].opened;
        let (mut value, mut pairs) = (0i32, 0u8);
        let mut laid = false;
        let mut only_own_melds = true;
        let mut used_taken = taken.is_none();
        for act in acts {
            if !matches!(act, Act::Meld(_)) {
                only_own_melds = false;
                if self.players[pi].opened == Opened::No && !self.settle_opening(p, value, pairs) {
                    return Err(());
                }
            }
            match act {
                Act::Meld(shape) => {
                    let tiles = shape.tiles();
                    if !self.holds(p, &tiles) {
                        return Err(());
                    }
                    let opened = self.players[pi].opened;
                    if shape.is_pair() {
                        if !r.pairs_enabled || opened == Opened::Melds || value > 0 {
                            return Err(());
                        }
                        if matches!(shape, Shape::GostergePair { .. }) && !self.players[pi].gosterge_bonus {
                            return Err(());
                        }
                        if start == Opened::Pairs {
                            if self.table.pair_room(p, &r) == 0 {
                                return Err(());
                            }
                            self.table.pairs_added[pi] += 1;
                        }
                        pairs += 1;
                    } else {
                        let too_long = matches!(shape, Shape::Run { .. }) && shape.len() > r.max_run_len;
                        let forbidden = match opened {
                            Opened::Pairs => !r.pair_opener_can_meld,
                            Opened::Melds => start != Opened::No && !r.new_melds_after_open,
                            Opened::No => pairs > 0,
                        };
                        if too_long || forbidden || shape.len() < r.meld_min_len {
                            return Err(());
                        }
                        value += shape.open_value(&r);
                    }
                    used_taken |= taken.is_some_and(|t| tiles.contains(&t));
                    self.place(p, p, *shape);
                    laid = true;
                }
                Act::Swap(slot) => {
                    if !r.joker_retrieve || !self.table.joker_slots(&r).contains(slot) || !self.holds(p, &slot.tiles) {
                        return Err(());
                    }
                    used_taken |= taken.is_some_and(|t| slot.tiles.contains(&t));
                    self.do_swap(p, slot);
                }
                Act::Layoff { meld, tile, side } => {
                    let allowed = self.players[pi].opened != Opened::Pairs || r.pair_opener_can_layoff;
                    if !allowed || self.players[pi].hand[*tile as usize] == 0 {
                        return Err(());
                    }
                    let before = self.table.melds.get(*meld).ok_or(())?.clone();
                    self.table.lay_off_on(*meld, *tile, Some(*side), &r).ok_or(())?;
                    self.players[pi].hand[*tile as usize] -= 1;
                    self.seen[*tile as usize] += 1;
                    used_taken |= taken == Some(*tile);
                    if let Some(log) = &mut self.log {
                        let stands_for = match (before.shape, side) {
                            (Shape::Run { color, lo, .. }, Side::Low) => pos_tile(color, lo - 1),
                            (Shape::Run { color, hi, .. }, Side::High) => pos_tile(color, hi + 1),
                            _ => *tile,
                        };
                        log.push(Step::Layoff {
                            tile: *tile,
                            owner: before.owner,
                            onto: before.shape.tiles(),
                            side: *side,
                            stands_for,
                        });
                    }
                }
                Act::Gift { owner, shape } => {
                    let tiles = shape.tiles();
                    let ok = shape.is_pair()
                        && r.pairs_onto_pair_opener
                        && self.players[pi].opened == Opened::Melds
                        && self.table.pair_owners().contains(owner)
                        && self.table.pair_room(*owner, &r) > 0
                        && self.holds(p, &tiles);
                    if !ok {
                        return Err(());
                    }
                    used_taken |= taken.is_some_and(|t| tiles.contains(&t));
                    self.table.pairs_added[*owner as usize] += 1;
                    self.place(p, *owner, *shape);
                }
            }
        }
        let short_opening = self.players[pi].opened == Opened::No && laid && !self.settle_opening(p, value, pairs);
        if start == Opened::No && self.players[pi].opened == Opened::Pairs {
            self.charge_big_pairs(p, pairs);
        }
        if !used_taken && r.take_must_use {
            return Err(());
        }
        Ok(ActsDone {
            short_opening,
            only_own_melds,
            pairs: pairs > 0,
        })
    }

    /// Play a whole turn: the steps, then the discard. Ok(Some) means the hand ended.
    pub fn play_move(&mut self, p: u8, mv: &Move, taken: Option<Tile>) -> Result<Option<Outcome>, ()> {
        let pi = p as usize;
        let was_unopened = self.players[pi].opened == Opened::No;
        let done = self.play_acts(p, &mv.acts, taken)?;
        if self.players[pi].hand[mv.discard as usize] == 0 {
            return Err(());
        }
        let finishing = self.hand_len(p) == 1;
        if done.short_opening {
            if !finishing || !done.only_own_melds || self.rules.elden_requires_open_min {
                return Err(());
            }
            self.players[pi].opened = if done.pairs { Opened::Pairs } else { Opened::Melds };
        }
        if finishing {
            let elden = was_unopened && (done.only_own_melds || self.rules.open_finish_is_elden);
            return Ok(Some(self.finish(p, mv.discard, elden)));
        }
        self.discard(p, mv.discard);
        Ok(None)
    }
}

/// Every meld whose lowest natural tile is `i`, built from `counts` (okeys may stand anywhere).
fn melds_with(counts: &Counts, i: Tile, rules: &Rules) -> Vec<Shape> {
    let (c, n) = (color(i), num(i));
    let jokers = counts[JOKER as usize];
    let min_len = rules.meld_min_len.max(2);
    let mut out = Vec::new();

    let others: Vec<u8> = ((c + 1)..NCOLORS).filter(|&c2| counts[id(c2, n) as usize] > 0).collect();
    for mask in 0u8..(1 << others.len()) {
        let mut colors = 1u8 << c;
        for (b, &c2) in others.iter().enumerate() {
            if mask >> b & 1 == 1 {
                colors |= 1 << c2;
            }
        }
        let naturals = colors.count_ones() as u8;
        for jk in 0..=jokers.min(4 - naturals) {
            if (min_len..=4).contains(&(naturals + jk)) {
                out.push(Shape::Group {
                    num: n,
                    colors,
                    jokers: jk,
                });
            }
        }
    }

    fn extend(counts: &Counts, c: u8, lo: u8, next: u8, left: u8, mask: u16, min_len: u8, max_len: u8, out: &mut Vec<Shape>) {
        let len = next - lo;
        if len >= min_len {
            out.push(Shape::Run {
                color: c,
                lo,
                hi: next - 1,
                jokers: mask,
            });
        }
        if len >= max_len || next > 13 {
            return;
        }
        if counts[id(c, next) as usize] > 0 {
            extend(counts, c, lo, next + 1, left, mask, min_len, max_len, out);
        }
        if left > 0 {
            extend(counts, c, lo, next + 1, left - 1, mask | 1 << next, min_len, max_len, out);
        }
    }
    let max_len = rules.max_run_len.max(min_len);
    for prefix in 0..=jokers.min(n - 1) {
        let lo = n - prefix;
        let mask = (lo..n).fold(0u16, |m, p| m | 1 << p);
        extend(counts, c, lo, n + 1, jokers - prefix, mask, min_len, max_len, &mut out);
    }
    out
}

fn potential(counts: &Counts, from: usize) -> i32 {
    (from..NNAT).map(|k| counts[k] as i32 * num(k as Tile) as i32).sum::<i32>() + counts[JOKER as usize] as i32 * 13
}

/// Every set of disjoint melds worth at least `min_value`, each set exactly once.
fn meld_sets(counts: &mut Counts, rules: &Rules, min_value: i32, cap: usize, out: &mut Vec<Vec<Shape>>) {
    fn rec(
        counts: &mut Counts,
        rules: &Rules,
        start: usize,
        floor: Option<(usize, Shape)>,
        cur: &mut Vec<Shape>,
        value: i32,
        min_value: i32,
        cap: usize,
        out: &mut Vec<Vec<Shape>>,
    ) {
        if out.len() >= cap {
            return;
        }
        let Some(i) = (start..NNAT).find(|&k| counts[k] > 0) else {
            if value >= min_value {
                out.push(cur.clone());
            }
            return;
        };
        if value + potential(counts, i) < min_value {
            return;
        }
        rec(counts, rules, i + 1, None, cur, value, min_value, cap, out);
        for m in melds_with(counts, i as Tile, rules) {
            if floor.is_some_and(|(at, f)| at == i && m < f) {
                continue;
            }
            let tiles = m.tiles();
            tiles.iter().for_each(|&t| counts[t as usize] -= 1);
            cur.push(m);
            rec(
                counts,
                rules,
                i,
                Some((i, m)),
                cur,
                value + m.open_value(rules),
                min_value,
                cap,
                out,
            );
            cur.pop();
            tiles.iter().for_each(|&t| counts[t as usize] += 1);
        }
    }
    rec(counts, rules, 0, None, &mut Vec::new(), 0, min_value, cap, out);
}

/// Pairs available in a hand: two identical tiles, or a tile with an okey.
fn pair_shapes(hand: &Counts, rules: &Rules) -> Vec<Shape> {
    let mut v = Vec::new();
    for t in 0..NNAT as Tile {
        if hand[t as usize] >= 2 {
            v.push(Shape::Pair { tile: t, joker: false });
        } else if hand[t as usize] == 1 && hand[JOKER as usize] > 0 && rules.joker_pairs_with_single {
            v.push(Shape::Pair { tile: t, joker: true });
        }
    }
    v
}

/// Subsets of `items` with between `min` and `max` members, at most `cap` of them.
fn subsets<T: Copy>(items: &[T], min: usize, max: usize, cap: usize) -> Vec<Vec<T>> {
    let n = items.len().min(16);
    let mut out = Vec::new();
    for mask in 0u32..(1 << n) {
        let k = mask.count_ones() as usize;
        if k >= min && k <= max {
            out.push((0..n).filter(|b| mask >> b & 1 == 1).map(|b| items[b]).collect());
            if out.len() >= cap {
                break;
            }
        }
    }
    out
}

pub struct Generated {
    /// Each entry is the steps before the discard; the discard can be any tile left in hand.
    pub positions: Vec<Vec<Act>>,
    /// True when the cap stopped the enumeration early.
    pub truncated: bool,
}

struct Gen<'a> {
    root: &'a Game,
    p: u8,
    taken: Option<Tile>,
    cap: usize,
    seen: HashMap<u64, usize>,
    out: Vec<Vec<Act>>,
    truncated: bool,
}

impl Gen<'_> {
    fn full(&mut self) -> bool {
        if self.out.len() >= self.cap {
            self.truncated = true;
        }
        self.truncated
    }

    /// Record a position if it is legal and leaves a tile to discard. Of several ways to the same position the
    /// one with the fewest steps is kept.
    fn push(&mut self, acts: &[Act]) {
        if self.full() {
            return;
        }
        let mut g = self.root.clone();
        if g.play_acts(self.p, acts, self.taken).is_err() || g.hand_len(self.p) == 0 {
            return;
        }
        let mut h = DefaultHasher::new();
        g.players[self.p as usize].hand.hash(&mut h);
        let mut melds: Vec<(u8, Shape)> = g.table.melds.iter().map(|m| (m.owner, m.shape)).collect();
        melds.sort_unstable();
        melds.hash(&mut h);
        g.players.iter().for_each(|pl| pl.penalty.hash(&mut h));
        match self.seen.get(&h.finish()) {
            Some(&at) if acts.len() < self.out[at].len() => self.out[at] = acts.to_vec(),
            Some(_) => {}
            None => {
                self.seen.insert(h.finish(), self.out.len());
                self.out.push(acts.to_vec());
            }
        }
    }

    /// Lay-offs: any number of tiles on any melds, in one canonical order.
    fn layoffs(&mut self, g: &Game, idx: usize, rank: u8, last: Tile, acts: &mut Vec<Act>) {
        if self.full() {
            return;
        }
        if idx >= g.table.melds.len() {
            self.gifts(g, acts);
            return;
        }
        self.layoffs(g, idx + 1, 0, 0, acts);
        let side_rank = |s: Side| match s {
            Side::Low => 0u8,
            Side::High => 1,
            Side::Group => 2,
        };
        for o in g.table.melds[idx].options(&g.rules) {
            let r = side_rank(o.side);
            if r < rank || (o.side == Side::Group && rank == 2 && o.tile <= last) || g.players[self.p as usize].hand[o.tile as usize] == 0 {
                continue;
            }
            let act = Act::Layoff {
                meld: idx,
                tile: o.tile,
                side: o.side,
            };
            let mut g2 = g.clone();
            if g2.play_acts(self.p, std::slice::from_ref(&act), None).is_err() {
                continue;
            }
            acts.push(act);
            self.layoffs_same(&g2, idx, r, o.tile, acts);
            acts.pop();
        }
    }

    /// Continue on the same meld (a second tile on an end) or move on.
    fn layoffs_same(&mut self, g: &Game, idx: usize, rank: u8, last: Tile, acts: &mut Vec<Act>) {
        if self.full() {
            return;
        }
        self.layoffs(g, idx + 1, 0, 0, acts);
        let side_rank = |s: Side| match s {
            Side::Low => 0u8,
            Side::High => 1,
            Side::Group => 2,
        };
        for o in g.table.melds[idx].options(&g.rules) {
            let r = side_rank(o.side);
            if r < rank || (o.side == Side::Group && o.tile <= last) || g.players[self.p as usize].hand[o.tile as usize] == 0 {
                continue;
            }
            let act = Act::Layoff {
                meld: idx,
                tile: o.tile,
                side: o.side,
            };
            let mut g2 = g.clone();
            if g2.play_acts(self.p, std::slice::from_ref(&act), None).is_err() {
                continue;
            }
            acts.push(act);
            self.layoffs_same(&g2, idx, r, o.tile, acts);
            acts.pop();
        }
    }

    /// Pairs handed to other players' pair areas.
    fn gifts(&mut self, g: &Game, acts: &mut Vec<Act>) {
        self.push(acts);
        let me = &g.players[self.p as usize];
        if me.opened != Opened::Melds || !g.rules.pairs_onto_pair_opener {
            return;
        }
        let owners: Vec<u8> = g.table.pair_owners();
        let room: usize = owners.iter().map(|&o| g.table.pair_room(o, &g.rules) as usize).sum();
        if room == 0 {
            return;
        }
        let pairs = pair_shapes(&me.hand, &g.rules);
        for set in subsets(&pairs, 1, room, 256) {
            let mut g2 = g.clone();
            let before = acts.len();
            let mut ok = true;
            for shape in set {
                let Some(&owner) = owners.iter().find(|&&o| g2.table.pair_room(o, &g2.rules) > 0) else {
                    ok = false;
                    break;
                };
                let act = Act::Gift { owner, shape };
                if g2.play_acts(self.p, std::slice::from_ref(&act), None).is_err() {
                    ok = false;
                    break;
                }
                acts.push(act);
            }
            if ok {
                self.push(acts);
            }
            acts.truncate(before);
        }
    }

    /// New melds (or a pair opener's own pairs) from what is in hand, then lay-offs.
    fn extra_melds(&mut self, g: &Game, acts: &mut Vec<Act>, enumerate: bool) {
        let me = &g.players[self.p as usize];
        let mut sets: Vec<Vec<Shape>> = vec![vec![]];
        if enumerate {
            match me.opened {
                Opened::Pairs => {
                    let room = g.table.pair_room(self.p, &g.rules) as usize;
                    sets.extend(subsets(&pair_shapes(&me.hand, &g.rules), 1, room, 256));
                    if g.rules.pair_opener_can_meld {
                        let mut h = me.hand;
                        meld_sets(&mut h, &g.rules, 1, self.cap, &mut sets);
                    }
                }
                _ => {
                    let mut h = me.hand;
                    sets.clear();
                    meld_sets(&mut h, &g.rules, 0, self.cap, &mut sets);
                }
            }
        }
        let can_layoff = me.opened == Opened::Melds || (me.opened == Opened::Pairs && g.rules.pair_opener_can_layoff);
        for set in sets {
            if self.full() {
                return;
            }
            let extra: Vec<Act> = set.into_iter().map(Act::Meld).collect();
            let mut g2 = g.clone();
            if g2.play_acts(self.p, &extra, None).is_err() {
                continue;
            }
            let before = acts.len();
            acts.extend(extra);
            if can_layoff {
                self.layoffs(&g2, 0, 0, 0, acts);
            } else {
                self.gifts(&g2, acts);
            }
            acts.truncate(before);
        }
    }

    /// Okey swaps in a canonical order, then everything else an opened player may do.
    fn swaps(&mut self, g: &Game, acts: &mut Vec<Act>, from: usize, swapped: bool, new_melds: bool) {
        if self.full() {
            return;
        }
        self.extra_melds(g, acts, new_melds || swapped);
        if !g.rules.joker_retrieve {
            return;
        }
        let hand = g.players[self.p as usize].hand;
        for slot in g.table.joker_slots(&g.rules) {
            if slot.meld < from || !slot.tiles.iter().all(|&t| hand[t as usize] > 0) {
                continue;
            }
            let act = Act::Swap(slot.clone());
            let mut g2 = g.clone();
            if g2.play_acts(self.p, std::slice::from_ref(&act), None).is_err() {
                continue;
            }
            acts.push(act);
            self.swaps(&g2, acts, slot.meld, true, new_melds);
            acts.pop();
        }
    }
}

/// Every distinct position player `p` can reach this turn before discarding.
pub fn generate(g: &Game, p: u8, taken: Option<Tile>, cap: usize) -> Generated {
    let mut me = Gen {
        root: g,
        p,
        taken,
        cap,
        seen: HashMap::new(),
        out: Vec::new(),
        truncated: false,
    };
    let hand = g.players[p as usize].hand;
    let mut acts = Vec::new();
    if g.players[p as usize].opened != Opened::No {
        me.swaps(g, &mut acts, 0, false, true);
        return Generated {
            positions: me.out,
            truncated: me.truncated,
        };
    }

    me.push(&[]);
    let mut openings: Vec<Vec<Shape>> = Vec::new();
    let mut h = hand;
    meld_sets(&mut h, &g.rules, g.meld_req, cap, &mut openings);
    if g.rules.pairs_enabled {
        let gost = g.players[p as usize].gosterge_bonus.then_some(g.gosterge);
        let pairs = pair_plan(&hand, &g.rules, gost).pairs;
        openings.extend(subsets(&pairs, g.pair_req as usize, pairs.len(), 512));
    }
    for set in openings {
        if me.full() {
            break;
        }
        acts = set.into_iter().map(Act::Meld).collect();
        let mut g2 = g.clone();
        if g2.play_acts(p, &acts, None).is_err() {
            continue;
        }
        // Supersets of this opening are separate openings, so only an okey swap can unlock new melds here.
        me.swaps(&g2, &mut acts, 0, false, false);
    }

    // Elden below the threshold: every tile but one in the player's own melds or pairs.
    if let Some((melds, _, _)) = g.elden_plan(&mut Solver::new(&g.rules), p, taken) {
        me.push(&melds.into_iter().map(Act::Meld).collect::<Vec<_>>());
    }
    Generated {
        positions: me.out,
        truncated: me.truncated,
    }
}
