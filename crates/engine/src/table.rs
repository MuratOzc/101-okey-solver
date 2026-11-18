use crate::meld::{Attach, Shape, pos_tile};
use crate::rules::Rules;
use crate::tile::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableMeld {
    pub owner: u8,
    pub shape: Shape,
    /// Tiles added to the low / high end during the current turn.
    #[serde(default)]
    pub added_lo: u8,
    #[serde(default)]
    pub added_hi: u8,
    /// Laid down during the current turn.
    #[serde(default)]
    pub fresh: bool,
    /// Okeys here that someone other than the owner laid: who, and the run position (0 outside runs).
    #[serde(default)]
    pub lent: [Option<(u8, u8)>; 2],
}

impl TableMeld {
    pub fn new(owner: u8, shape: Shape) -> Self {
        TableMeld {
            owner,
            shape,
            added_lo: 0,
            added_hi: 0,
            fresh: false,
            lent: [None; 2],
        }
    }

    pub fn laid(owner: u8, shape: Shape) -> Self {
        TableMeld {
            fresh: true,
            ..TableMeld::new(owner, shape)
        }
    }

    // A run laid this turn and grown past the longest new run would be that longer run laid at once.
    fn full(&self, rules: &Rules) -> bool {
        self.fresh && self.shape.len() >= rules.max_run_len
    }

    /// Record that `by` laid the okey now at that end of the run, or in the group or pair.
    pub fn okey_from(&mut self, by: u8, side: Side) {
        if by == self.owner {
            return;
        }
        let at = match (self.shape, side) {
            (Shape::Run { lo, .. }, Side::Low) => lo,
            (Shape::Run { hi, .. }, Side::High) => hi,
            _ => 0,
        };
        if let Some(free) = self.lent.iter_mut().find(|l| l.is_none()) {
            *free = Some((by, at));
        }
    }

    /// Who laid each okey of this meld.
    pub fn okey_layers(&self) -> Vec<u8> {
        let mut by: Vec<u8> = self.lent.iter().flatten().map(|l| l.0).collect();
        by.resize((self.shape.jokers() as usize).max(by.len()), self.owner);
        by
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Table {
    pub melds: Vec<TableMeld>,
    /// Pairs added to each player's pair area during the current turn.
    #[serde(default)]
    pub pairs_added: [u8; 4],
}

/// Swapping okeys out of a table meld: put `tiles` in, take `jokers` okeys back.
/// With `group_joker_must_complete`, a group must hold four tiles after the swap.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JokerSlot {
    pub meld: usize,
    pub tiles: Vec<Tile>,
    pub jokers: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Low,
    High,
    Group,
}

/// One tile that fits a table meld. For an okey, `stands_for` is the tile it replaces
/// (JOKER itself when a group leaves that open).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayOption {
    pub tile: Tile,
    pub side: Side,
    pub stands_for: Tile,
}

pub(crate) fn run_top(rules: &Rules, lo: u8) -> u8 {
    if rules.one_after_thirteen && lo > 1 { 14 } else { 13 }
}

impl Table {
    pub fn new_turn(&mut self) {
        for m in &mut self.melds {
            m.added_lo = 0;
            m.added_hi = 0;
            m.fresh = false;
        }
        self.pairs_added = [0; 4];
    }

    pub fn pair_owners(&self) -> Vec<u8> {
        let mut v: Vec<u8> = self.melds.iter().filter(|m| m.shape.is_pair()).map(|m| m.owner).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    pub fn pair_room(&self, owner: u8, rules: &Rules) -> u8 {
        rules.pairs_per_turn.saturating_sub(self.pairs_added[owner as usize])
    }

    pub fn attach(&self, rules: &Rules) -> Attach {
        let mut a = Attach::none();
        for m in &self.melds {
            m.each_option(rules, |o| a.tile[o.tile as usize] = true);
        }
        a
    }

    /// What throwing `t` costs a player who still holds tiles afterwards.
    pub fn discard_penalty(&self, t: Tile, opened: bool, rules: &Rules) -> i32 {
        if t == JOKER {
            rules.penalty_discard_joker
        } else if (opened || rules.playable_applies_unopened) && self.is_playable(t, rules) {
            rules.penalty_playable
        } else {
            0
        }
    }

    /// Playable: a tile that fits a table meld regardless of this turn's lay-off counters.
    pub fn is_playable(&self, t: Tile, rules: &Rules) -> bool {
        if t == JOKER {
            return false;
        }
        self.melds.iter().any(|m| match m.shape {
            Shape::Run { color: c, lo, hi, .. } => {
                color(t) == c && ((lo > 1 && pos_tile(c, lo - 1) == t) || (hi < run_top(rules, lo) && pos_tile(c, hi + 1) == t))
            }
            Shape::Group { num: n, colors, jokers } => colors.count_ones() as u8 + jokers < 4 && num(t) == n && colors >> color(t) & 1 == 0,
            _ => false,
        })
    }

    pub fn joker_slots(&self, rules: &Rules) -> Vec<JokerSlot> {
        let mut v = Vec::new();
        for (i, m) in self.melds.iter().enumerate() {
            match m.shape {
                Shape::Run { color, lo, hi, jokers } => {
                    for p in lo..=hi {
                        if jokers >> p & 1 == 1 {
                            v.push(JokerSlot {
                                meld: i,
                                tiles: vec![pos_tile(color, p)],
                                jokers: 1,
                            });
                        }
                    }
                }
                Shape::Group { num, colors, jokers } if jokers > 0 => {
                    let missing: Vec<Tile> = (0..NCOLORS).filter(|c| colors >> c & 1 == 0).map(|c| id(c, num)).collect();
                    let len = colors.count_ones() as u8 + jokers;
                    let fill = if rules.group_joker_must_complete { 4 - len } else { 0 };
                    for take in 1..=jokers {
                        let k = (take + fill) as usize;
                        for mask in 0u8..(1 << missing.len()) {
                            if mask.count_ones() as usize == k {
                                let tiles = (0..missing.len()).filter(|b| mask >> b & 1 == 1).map(|b| missing[b]).collect();
                                v.push(JokerSlot {
                                    meld: i,
                                    tiles,
                                    jokers: take,
                                });
                            }
                        }
                    }
                }
                Shape::Pair { tile, joker: true } if tile != JOKER => {
                    v.push(JokerSlot {
                        meld: i,
                        tiles: vec![tile],
                        jokers: 1,
                    });
                }
                _ => {}
            }
        }
        v
    }

    /// Put the slot's natural tiles into the meld, removing its okeys. Returns who had laid each okey
    /// `taker` gets; where the okeys cannot be told apart, the taker's own come first.
    pub fn swap_joker(&mut self, slot: &JokerSlot, taker: u8) -> [Option<u8>; 2] {
        let m = &mut self.melds[slot.meld];
        let mut at = 0;
        match &mut m.shape {
            Shape::Run { jokers, color, .. } => {
                let Some(p) = (1..=14u8).find(|&p| *jokers >> p & 1 == 1 && pos_tile(*color, p) == slot.tiles[0]) else {
                    return [None; 2];
                };
                *jokers &= !(1 << p);
                at = p;
            }
            Shape::Group { colors, jokers, .. } => {
                for &t in &slot.tiles {
                    *colors |= 1 << color(t);
                }
                *jokers -= slot.jokers;
            }
            Shape::Pair { joker, .. } => *joker = false,
            _ => return [None; 2],
        }
        let mut from = [None; 2];
        for f in from.iter_mut().take(slot.jokers as usize) {
            let here = |l: &Option<(u8, u8)>| l.is_some_and(|l| l.1 == at);
            let pick = (0..2)
                .find(|&k| here(&m.lent[k]) && m.lent[k].is_some_and(|l| l.0 == taker))
                .or_else(|| (0..2).find(|&k| here(&m.lent[k])));
            *f = Some(match pick {
                Some(k) => m.lent[k].take().map_or(m.owner, |l| l.0),
                None => m.owner,
            });
        }
        from
    }

    /// Lay a single tile onto the first meld it extends, respecting per-turn end limits.
    /// An okey goes to a group when one has room; `okey_on_runs` false keeps it off runs entirely.
    pub fn lay_off(&mut self, t: Tile, rules: &Rules, okey_on_runs: bool) -> Option<(usize, Side)> {
        let is_group = |m: &TableMeld| matches!(m.shape, Shape::Group { .. });
        let mut order: Vec<usize> = (0..self.melds.len()).collect();
        if t == JOKER {
            order.sort_by_key(|&i| !is_group(&self.melds[i]));
            if !okey_on_runs {
                order.retain(|&i| is_group(&self.melds[i]));
            }
        }
        order
            .into_iter()
            .find_map(|i| lay_off_meld(&mut self.melds[i], t, rules, None).map(|s| (i, s)))
    }

    pub fn group_has_room(&self) -> bool {
        self.melds
            .iter()
            .any(|m| matches!(m.shape, Shape::Group { colors, jokers, .. } if colors.count_ones() as u8 + jokers < 4))
    }

    /// Lay `t` onto meld `meld`; `side` picks the end (needed for an okey on a run).
    pub fn lay_off_on(&mut self, meld: usize, t: Tile, side: Option<Side>, rules: &Rules) -> Option<Side> {
        self.melds.get_mut(meld).and_then(|m| lay_off_meld(m, t, rules, side))
    }
}

impl TableMeld {
    /// Every single tile that can be laid on this meld right now, and where.
    pub fn options(&self, rules: &Rules) -> Vec<LayOption> {
        let mut v = Vec::new();
        self.each_option(rules, |o| v.push(o));
        v
    }

    fn each_option(&self, rules: &Rules, mut found: impl FnMut(LayOption)) {
        let cap = rules.layoff_per_end;
        let mut fits = |tile, side, stands_for| found(LayOption { tile, side, stands_for });
        match self.shape {
            Shape::Run { color, lo, hi, .. } => {
                if lo > 1 && self.added_lo < cap && !self.full(rules) {
                    let t = pos_tile(color, lo - 1);
                    fits(t, Side::Low, t);
                    fits(JOKER, Side::Low, t);
                }
                if hi < run_top(rules, lo) && self.added_hi < cap && !self.full(rules) {
                    let t = pos_tile(color, hi + 1);
                    fits(t, Side::High, t);
                    fits(JOKER, Side::High, t);
                }
            }
            Shape::Group { num, colors, jokers } if colors.count_ones() as u8 + jokers < 4 => {
                for c in (0..NCOLORS).filter(|c| colors >> c & 1 == 0) {
                    fits(id(c, num), Side::Group, id(c, num));
                }
                fits(JOKER, Side::Group, JOKER);
            }
            _ => {}
        }
    }
}

fn lay_off_meld(m: &mut TableMeld, t: Tile, rules: &Rules, want: Option<Side>) -> Option<Side> {
    let cap = rules.layoff_per_end;
    let full = m.full(rules);
    match &mut m.shape {
        Shape::Run { color: c, lo, hi, jokers } => {
            if t != JOKER && color(t) != *c {
                return None;
            }
            let top = run_top(rules, *lo);
            let lo_ok = *lo > 1 && m.added_lo < cap && !full && want != Some(Side::High);
            let hi_ok = *hi < top && m.added_hi < cap && !full && want != Some(Side::Low);
            if hi_ok && (t == JOKER || pos_tile(*c, *hi + 1) == t) {
                *hi += 1;
                if t == JOKER {
                    *jokers |= 1 << *hi;
                }
                m.added_hi += 1;
                Some(Side::High)
            } else if lo_ok && (t == JOKER || pos_tile(*c, *lo - 1) == t) {
                *lo -= 1;
                if t == JOKER {
                    *jokers |= 1 << *lo;
                }
                m.added_lo += 1;
                Some(Side::Low)
            } else {
                None
            }
        }
        Shape::Group { num: n, colors, jokers } => {
            if colors.count_ones() as u8 + *jokers >= 4 || want.is_some_and(|w| w != Side::Group) {
                return None;
            }
            if t == JOKER {
                *jokers += 1;
                return Some(Side::Group);
            }
            if num(t) == *n && *colors >> color(t) & 1 == 0 {
                *colors |= 1 << color(t);
                return Some(Side::Group);
            }
            None
        }
        _ => None,
    }
}

/// Classify a list of tiles the user laid down into a meld shape.
pub fn classify(tiles: &[Tile], rules: &Rules) -> Option<Shape> {
    let jokers = tiles.iter().filter(|&&t| t == JOKER).count() as u8;
    let nat: Vec<Tile> = tiles.iter().copied().filter(|&t| t != JOKER).collect();
    if tiles.len() == 2 {
        return match (nat.len(), jokers) {
            (2, 0) if nat[0] == nat[1] => Some(Shape::Pair {
                tile: nat[0],
                joker: false,
            }),
            (1, 1) if rules.joker_pairs_with_single => Some(Shape::Pair { tile: nat[0], joker: true }),
            (0, 2) if rules.two_jokers_pair => Some(Shape::Pair { tile: JOKER, joker: true }),
            _ => None,
        };
    }
    if nat.is_empty() || tiles.len() < rules.meld_min_len as usize {
        return None;
    }
    let same_num = nat.iter().all(|&t| num(t) == num(nat[0]));
    let mut colors = 0u8;
    let distinct_colors = nat.iter().all(|&t| {
        let b = 1 << color(t);
        let ok = colors & b == 0;
        colors |= b;
        ok
    });
    let group = (same_num && distinct_colors && tiles.len() <= 4).then_some(Shape::Group {
        num: num(nat[0]),
        colors,
        jokers,
    });

    let same_color = nat.iter().all(|&t| color(t) == color(nat[0]));
    let run = if same_color { run_from(tiles, color(nat[0]), rules) } else { None };
    match (run, group) {
        (Some(r), Some(g)) => Some(if nat.len() == 1 { g } else { r }),
        (r, g) => r.or(g),
    }
}

/// The run as the tiles were entered, rising or falling, so an okey entered first or last stands
/// at that end. None when the order does not spell a run.
fn in_tap_order(tiles: &[Tile], c: u8, rules: &Rules) -> Option<Shape> {
    let len = tiles.len() as i32;
    let at = |high_one: bool| -> Vec<(i32, i32)> {
        tiles
            .iter()
            .enumerate()
            .filter(|&(_, &t)| t != JOKER)
            .map(|(i, &t)| (i as i32, if high_one && num(t) == 1 { 14 } else { num(t) as i32 }))
            .collect()
    };
    for high_one in [false, true] {
        if high_one && !rules.one_after_thirteen {
            break;
        }
        let nat = at(high_one);
        if nat.len() < 2 {
            return None;
        }
        for step in [1, -1] {
            let lo = if step == 1 {
                nat[0].1 - nat[0].0
            } else {
                nat[0].1 - (len - 1 - nat[0].0)
            };
            let hi = lo + len - 1;
            let fits = nat.iter().all(|&(i, p)| p == if step == 1 { lo + i } else { hi - i });
            if fits && lo >= 1 && hi <= run_top(rules, lo as u8) as i32 {
                let mut jokers = 0u16;
                for (i, &t) in tiles.iter().enumerate() {
                    if t == JOKER {
                        let p = if step == 1 { lo + i as i32 } else { hi - i as i32 };
                        jokers |= 1 << p;
                    }
                }
                return Some(Shape::Run {
                    color: c,
                    lo: lo as u8,
                    hi: hi as u8,
                    jokers,
                });
            }
        }
    }
    None
}

fn run_from(tiles: &[Tile], c: u8, rules: &Rules) -> Option<Shape> {
    let len = tiles.len() as u8;
    let mut positions: Vec<u8> = tiles.iter().filter(|&&t| t != JOKER).map(|&t| num(t)).collect();
    positions.sort_unstable();
    let try_layout = |pos: &[u8]| -> Option<Shape> {
        if pos.windows(2).any(|w| w[0] == w[1]) {
            return None;
        }
        let first = pos[0];
        let last = *pos.last().unwrap();
        if last - first + 1 > len {
            return None;
        }
        let top = if first == 1 {
            13
        } else if rules.one_after_thirteen {
            14
        } else {
            13
        };
        let hi = (first + len - 1).min(top);
        let lo = hi + 1 - len;
        if lo < 1 || lo > first || hi < last {
            return None;
        }
        let mut mask = 0u16;
        for p in lo..=hi {
            if !pos.contains(&p) {
                mask |= 1 << p;
            }
        }
        Some(Shape::Run {
            color: c,
            lo,
            hi,
            jokers: mask,
        })
    };
    if let Some(s) = in_tap_order(tiles, c, rules) {
        return Some(s);
    }
    if let Some(s) = try_layout(&positions) {
        return Some(s);
    }
    if rules.one_after_thirteen && positions.first() == Some(&1) {
        let mut p2: Vec<u8> = positions.iter().map(|&p| if p == 1 { 14 } else { p }).collect();
        p2.sort_unstable();
        return try_layout(&p2);
    }
    None
}
