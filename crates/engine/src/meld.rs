use crate::rules::Rules;
use crate::tile::*;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Shape {
    /// Positions lo..=hi; position 14 is the 1 placed after 13. Bit p of `jokers` marks an okey at p.
    Run {
        color: u8,
        lo: u8,
        hi: u8,
        jokers: u16,
    },
    Group {
        num: u8,
        colors: u8,
        jokers: u8,
    },
    /// `tile == JOKER` with `joker == true` is two okeys.
    Pair {
        tile: Tile,
        joker: bool,
    },
    GostergePair {
        gosterge: Tile,
        other: Tile,
    },
}

#[inline]
pub fn pos_tile(color: u8, p: u8) -> Tile {
    id(color, if p == 14 { 1 } else { p })
}

/// The tiles of one meld, held inline: the solver builds millions of these.
#[derive(Clone, Copy)]
pub struct TileList {
    tiles: [Tile; 14],
    len: u8,
}

impl TileList {
    #[inline]
    fn push(&mut self, t: Tile) {
        self.tiles[self.len as usize] = t;
        self.len += 1;
    }
}

impl std::ops::Deref for TileList {
    type Target = [Tile];
    #[inline]
    fn deref(&self) -> &[Tile] {
        &self.tiles[..self.len as usize]
    }
}

impl Shape {
    #[inline]
    pub fn list(&self) -> TileList {
        let mut l = TileList { tiles: [0; 14], len: 0 };
        match *self {
            Shape::Run { color, lo, hi, jokers } => {
                for p in lo..=hi {
                    l.push(if jokers >> p & 1 == 1 { JOKER } else { pos_tile(color, p) });
                }
            }
            Shape::Group { num, colors, jokers } => {
                for c in 0..4 {
                    if colors >> c & 1 == 1 {
                        l.push(id(c, num));
                    }
                }
                for _ in 0..jokers {
                    l.push(JOKER);
                }
            }
            Shape::Pair { tile, joker } => {
                l.push(tile);
                l.push(if tile == JOKER || joker { JOKER } else { tile });
            }
            Shape::GostergePair { gosterge, other } => {
                l.push(gosterge);
                l.push(other);
            }
        }
        l
    }

    pub fn tiles(&self) -> Vec<Tile> {
        self.list().to_vec()
    }

    pub fn len(&self) -> u8 {
        match *self {
            Shape::Run { lo, hi, .. } => hi - lo + 1,
            Shape::Group { colors, jokers, .. } => colors.count_ones() as u8 + jokers,
            _ => 2,
        }
    }

    pub fn is_pair(&self) -> bool {
        matches!(self, Shape::Pair { .. } | Shape::GostergePair { .. })
    }

    pub fn jokers(&self) -> u8 {
        match *self {
            Shape::Run { jokers, .. } => jokers.count_ones() as u8,
            Shape::Group { jokers, .. } => jokers,
            Shape::Pair { tile, joker } => {
                if tile == JOKER {
                    2
                } else {
                    joker as u8
                }
            }
            Shape::GostergePair { .. } => 0,
        }
    }

    pub fn open_value(&self, rules: &Rules) -> i32 {
        match *self {
            Shape::Run { lo, hi, .. } => (lo..=hi).map(|p| if p == 14 { rules.high_one_value } else { p as i32 }).sum(),
            Shape::Group { num, .. } => num as i32 * self.len() as i32,
            _ => 0,
        }
    }

    pub fn uses(&self, t: Tile) -> bool {
        self.list().contains(&t)
    }
}

#[inline]
pub fn penalty_of(t: Tile, rules: &Rules) -> i32 {
    if t == JOKER { rules.joker_in_hand } else { num(t) as i32 }
}

pub fn hand_penalty(c: &Counts, rules: &Rules) -> i32 {
    (0..NIDS).map(|i| c[i] as i32 * penalty_of(i as Tile, rules)).sum()
}

/// Whether `t` could sit in any run or group at all, given the rest of the hand. Okeys are counted
/// generously (all of them for every tile), so false is certain and true is only "maybe".
pub fn coverable(hand: &Counts, t: Tile, rules: &Rules) -> bool {
    let (c, n) = (color(t), num(t));
    let jokers = hand[JOKER as usize];
    let min_len = rules.meld_min_len.max(2);
    let mates = (0..NCOLORS).filter(|&c2| c2 != c && hand[id(c2, n) as usize] > 0).count() as u8;
    if 1 + mates + jokers >= min_len {
        return true;
    }
    let top = if rules.one_after_thirteen { 14 } else { 13 };
    let window_through = |at: u8| {
        (at.saturating_sub(min_len - 1).max(1)..=at).any(|a| {
            let b = a + min_len - 1;
            b <= top && (a..=b).filter(|&p| p == at || hand[pos_tile(c, p) as usize] > 0).count() as u8 + jokers >= min_len
        })
    };
    window_through(n) || (n == 1 && rules.one_after_thirteen && window_through(14))
}

/// More than the opening total any arrangement of this hand can reach.
pub fn open_bound(hand: &Counts, rules: &Rules) -> i32 {
    let high = 13.max(rules.high_one_value);
    let mut bound = hand[JOKER as usize] as i32 * high;
    for t in 0..NNAT as Tile {
        if hand[t as usize] > 0 && coverable(hand, t, rules) {
            let worth = if num(t) == 1 && rules.one_after_thirteen {
                1.max(rules.high_one_value)
            } else {
                num(t) as i32
            };
            bound += hand[t as usize] as i32 * worth;
        }
    }
    bound
}

/// How many tiles can certainly not be melded, counting up to `limit`.
pub fn stranded(hand: &Counts, rules: &Rules, limit: u32) -> u32 {
    let mut n = 0;
    for t in 0..NNAT as Tile {
        if hand[t as usize] > 0 && !coverable(hand, t, rules) {
            n += hand[t as usize] as u32;
            if n >= limit {
                break;
            }
        }
    }
    n
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Melds score by the value they represent (opening total).
    Open,
    /// Every tile scores what it would cost if left in hand.
    Penalty,
}

#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub mode: Mode,
    pub cover_w: i32,
    pub must: Option<Tile>,
    pub allow_melds: bool,
    pub pair_dump: bool,
    pub pair_cap: u8,
    /// False keeps okeys out of runs.
    pub joker_runs: bool,
}

impl Params {
    /// For the arrangement worth most as an opening.
    pub fn open(must: Option<Tile>) -> Self {
        Params {
            mode: Mode::Open,
            cover_w: 0,
            must,
            allow_melds: true,
            pair_dump: false,
            pair_cap: 0,
            joker_runs: true,
        }
    }

    /// For the arrangement that leaves the fewest points in hand.
    pub fn cover(must: Option<Tile>) -> Self {
        Params {
            mode: Mode::Penalty,
            cover_w: 1000,
            ..Self::open(must)
        }
    }
}

/// Tiles that can be laid off onto the table as single tiles. Index JOKER means an okey fits somewhere.
#[derive(Clone, Copy, Debug)]
pub struct Attach {
    pub tile: [bool; NIDS],
}

impl Default for Attach {
    fn default() -> Self {
        Attach::none()
    }
}

impl Attach {
    pub fn none() -> Self {
        Attach { tile: [false; NIDS] }
    }

    #[inline]
    pub fn fits(&self, t: Tile) -> bool {
        self.tile[t as usize]
    }

    fn bits(&self) -> u64 {
        self.tile.iter().enumerate().fold(0, |b, (i, &on)| b | (on as u64) << i)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub score: i32,
    pub melds: Vec<Shape>,
    pub layoffs: Vec<Tile>,
    pub rest: Vec<Tile>,
}

impl Plan {
    pub fn open_value(&self, rules: &Rules) -> i32 {
        self.melds.iter().map(|m| m.open_value(rules)).sum()
    }
    pub fn used(&self) -> usize {
        self.melds.iter().map(|m| m.len() as usize).sum::<usize>() + self.layoffs.len()
    }
}

#[derive(Clone, Copy, Debug)]
enum Choice {
    Leaf,
    Dead(Tile),
    Layoff(Tile),
    Meld(Shape),
}

const NEG: i32 = i32::MIN / 4;
const FLAG_BIT: u32 = 120;
const DUMP_SHIFT: u32 = 121;
/// The natural tiles' share of the packed counts (two bits each).
const NATURALS: u128 = (1 << (2 * NNAT)) - 1;
const CACHE_LIMIT: usize = 1 << 16;
/// Hands that fall into more independent parts than this are searched whole.
const MAX_PARTS: usize = 24;
const KNOWN: usize = 16;

/// Everything a search result depends on besides the rules.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Question {
    hand: u128,
    attach: u64,
    cover_w: i32,
    must: u8,
    pair_cap: u8,
    flags: u8,
}

pub struct Solver {
    memo: FxHashMap<u128, (i32, Choice)>,
    /// Answers to whole searches: within a turn, and across simulated deals, the same hand is asked about again and again.
    cache: FxHashMap<Question, Option<Rc<Plan>>>,
    /// Recently measured hands: (hand, tiles left over by the best cover, best opening total).
    /// A hand one tile away cannot be much better, which settles most "could this open / finish?" checks.
    known: [(Counts, Option<u8>, Option<i32>); KNOWN],
    known_next: usize,
    scratch: Vec<Shape>,
    counts: Counts,
    key: u128,
    flag: bool,
    dumps: u8,
    p: Params,
    attach: Attach,
    rules: Rules,
}

impl Solver {
    pub fn new(rules: &Rules) -> Self {
        Solver {
            memo: FxHashMap::default(),
            cache: FxHashMap::default(),
            known: [([0; NIDS], None, None); KNOWN],
            known_next: 0,
            scratch: Vec::new(),
            counts: [0; NIDS],
            key: 0,
            flag: false,
            dumps: 0,
            p: Params::open(None),
            attach: Attach::none(),
            rules: rules.clone(),
        }
    }

    fn remember(&mut self, hand: &Counts, left_over: Option<u8>, value: Option<i32>) {
        if let Some(e) = self.known.iter_mut().find(|e| e.0 == *hand) {
            e.1 = e.1.or(left_over);
            e.2 = e.2.or(value);
            return;
        }
        self.known[self.known_next] = (*hand, left_over, value);
        self.known_next = (self.known_next + 1) % KNOWN;
    }

    /// Record how many tiles the best arrangement of `hand` leaves unmelded (no lay-offs, no pairs).
    pub fn note_left_over(&mut self, hand: &Counts, left_over: usize) {
        self.remember(hand, Some(left_over.min(255) as u8), None);
    }

    /// Record the best opening total of `hand`.
    pub fn note_value(&mut self, hand: &Counts, value: i32) {
        self.remember(hand, None, Some(value));
    }

    /// At least this many tiles stay unmelded in any arrangement of `hand`.
    /// One more tile can rescue at most a whole meld's worth of others; one fewer frees at most itself.
    pub fn left_over_at_least(&self, hand: &Counts) -> u32 {
        let per_added = self.rules.max_run_len.max(4) as i32 - 1;
        let mut best = 0;
        for (seen, left, _) in &self.known {
            let Some(left) = left else { continue };
            let (mut added, mut removed) = (0i32, 0i32);
            for i in 0..NIDS {
                let d = hand[i] as i32 - seen[i] as i32;
                if d > 0 { added += d } else { removed -= d }
            }
            best = best.max(*left as i32 - per_added * added - removed);
        }
        best.max(0) as u32
    }

    /// No arrangement of `hand` opens for more than this.
    /// Each added tile brings at most the richest meld it could sit in; removing tiles never helps.
    pub fn value_at_most(&self, hand: &Counts) -> i32 {
        let len = self.rules.max_run_len.max(4) as i32;
        // The richest meld a tile numbered `n` can sit in: a full-length run reaching as high as it can, or four of a kind.
        let richest = |n: i32| {
            if self.rules.one_after_thirteen {
                return len * 13.max(self.rules.high_one_value);
            }
            let top = (n + len - 1).min(13);
            (top - len + 1..=top).sum::<i32>().max(4 * n)
        };
        let mut best = i32::MAX;
        for (seen, _, value) in &self.known {
            let Some(value) = value else { continue };
            let mut bound = *value;
            for i in 0..NIDS {
                let added = (hand[i] as i32 - seen[i] as i32).max(0);
                if added > 0 {
                    let n = if i == JOKER as usize { 13 } else { num(i as Tile) as i32 };
                    bound += added * richest(n);
                }
            }
            best = best.min(bound);
        }
        best
    }

    pub fn run(&mut self, hand: &Counts, p: Params, attach: Option<&Attach>) -> Option<Plan> {
        let attach = attach.copied().unwrap_or_else(Attach::none);
        // A cap on pairs, or an okey that must be used, ties the whole hand together.
        if p.pair_dump || p.must == Some(JOKER) {
            return self.whole(hand, p, attach).map(|plan| (*plan).clone());
        }
        self.split(hand, p, attach)
    }

    /// The same answer without splitting the hand: the reference the split search is checked against.
    pub fn run_whole(&mut self, hand: &Counts, p: Params, attach: Option<&Attach>) -> Option<Plan> {
        let attach = attach.copied().unwrap_or_else(Attach::none);
        self.whole(hand, p, attach).map(|plan| (*plan).clone())
    }

    /// One search over the given tiles as a single block, answered from the cache when it can be.
    fn whole(&mut self, hand: &Counts, p: Params, attach: Attach) -> Option<Rc<Plan>> {
        let key = (0..NIDS).fold(0u128, |k, i| k | (hand[i] as u128) << (2 * i));
        let question = Question {
            hand: key,
            attach: attach.bits(),
            cover_w: p.cover_w,
            must: p.must.unwrap_or(u8::MAX),
            pair_cap: p.pair_cap,
            // Without an okey in hand the okey-in-runs switch cannot matter, so both settings share one answer.
            flags: (p.mode == Mode::Open) as u8
                | (p.allow_melds as u8) << 1
                | (p.pair_dump as u8) << 2
                | ((p.joker_runs || hand[JOKER as usize] == 0) as u8) << 3,
        };
        if let Some(known) = self.cache.get(&question) {
            return known.clone();
        }
        let plan = self.search(hand, key, p, attach).map(Rc::new);
        if self.cache.len() >= CACHE_LIMIT {
            self.cache.clear();
        }
        self.cache.insert(question, plan.clone());
        plan
    }

    /// Tiles that can never share a meld are solved apart: two tiles are linked when they have the
    /// same number, or the same color close enough for a run (okeys can bridge a gap). Each part is
    /// tiny, and the same parts keep turning up from turn to turn and deal to deal, so most answers
    /// are already in the cache. Okeys are handed out among the parts in the best way.
    fn split(&mut self, hand: &Counts, p: Params, attach: Attach) -> Option<Plan> {
        let jokers = hand[JOKER as usize] as usize;
        let reach = 1 + if p.joker_runs { jokers as u8 } else { 0 };
        let mut parent = [0u8; NNAT];
        for (i, slot) in parent.iter_mut().enumerate() {
            *slot = i as u8;
        }
        fn root(parent: &mut [u8; NNAT], mut x: u8) -> u8 {
            while parent[x as usize] != x {
                parent[x as usize] = parent[parent[x as usize] as usize];
                x = parent[x as usize];
            }
            x
        }
        let join = |parent: &mut [u8; NNAT], a: Tile, b: Tile| {
            let (ra, rb) = (root(parent, a), root(parent, b));
            parent[ra.max(rb) as usize] = ra.min(rb);
        };
        // With an okey around, anything close enough might meld. Without one, two tiles are linked
        // only if a real meld holds them both, which leaves most of a typical hand in tiny parts.
        let min_len = self.rules.meld_min_len.max(2);
        let exact = jokers == 0;
        let top = if self.rules.one_after_thirteen { 14 } else { 13 };
        for c in 0..NCOLORS {
            let has = |pos: u8| (1..=top).contains(&pos) && hand[pos_tile(c, pos) as usize] > 0;
            let mut last: Option<(u8, Tile)> = None;
            for pos in 1..=top {
                if !has(pos) {
                    continue;
                }
                let t = pos_tile(c, pos);
                if let Some((at, prev)) = last
                    && pos - at <= reach
                    && (!exact || min_len <= 2 || has(at.wrapping_sub(1)) || has(pos + 1))
                {
                    join(&mut parent, prev, t);
                }
                last = Some((pos, t));
            }
        }
        for n in 1..=13u8 {
            let colors = (0..NCOLORS).filter(|&c| hand[id(c, n) as usize] > 0).count() as u8;
            if exact && colors < min_len {
                continue;
            }
            let mut first: Option<Tile> = None;
            for c in 0..NCOLORS {
                let t = id(c, n);
                if hand[t as usize] > 0 {
                    match first {
                        Some(f) => join(&mut parent, f, t),
                        None => first = Some(t),
                    }
                }
            }
        }

        // A tile linked to nothing (when links are exact) can only be laid off or left over.
        let mut size = [0u8; NNAT];
        for t in 0..NNAT as Tile {
            if hand[t as usize] > 0 {
                size[root(&mut parent, t) as usize] += 1;
            }
        }
        let mut loose = Plan::default();
        let mut roots = [0u8; MAX_PARTS];
        let mut parts = [[0u8; NIDS]; MAX_PARTS];
        let mut masks = [Attach::none(); MAX_PARTS];
        let mut count = 0;
        for t in 0..NNAT as Tile {
            if hand[t as usize] == 0 {
                continue;
            }
            let r = root(&mut parent, t);
            if exact && size[r as usize] == 1 {
                let copies = hand[t as usize] as usize;
                if attach.fits(t) {
                    loose.score += copies as i32 * (penalty_of(t, &self.rules) + p.cover_w);
                    loose.layoffs.extend(std::iter::repeat_n(t, copies));
                } else if p.must == Some(t) {
                    return None;
                } else {
                    loose.rest.extend(std::iter::repeat_n(t, copies));
                }
                continue;
            }
            let k = match roots[..count].iter().position(|&x| x == r) {
                Some(k) => k,
                None if count < MAX_PARTS => {
                    roots[count] = r;
                    count += 1;
                    count - 1
                }
                None => return self.whole(hand, p, attach).map(|plan| (*plan).clone()),
            };
            parts[k][t as usize] = hand[t as usize];
            masks[k].tile[t as usize] = attach.tile[t as usize];
        }
        if count == 0 && jokers > 0 {
            return self.whole(hand, p, attach).map(|plan| (*plan).clone());
        }

        // answers[k][j]: part k solved with j okeys.
        let mut answers: Vec<[Option<Rc<Plan>>; 3]> = Vec::with_capacity(count);
        for k in 0..count {
            let mut row: [Option<Rc<Plan>>; 3] = [None, None, None];
            let must = p.must.filter(|&m| parts[k][m as usize] > 0);
            masks[k].tile[JOKER as usize] = attach.tile[JOKER as usize];
            for (j, slot) in row.iter_mut().enumerate().take(jokers.min(2) + 1) {
                parts[k][JOKER as usize] = j as u8;
                *slot = self.whole(&parts[k], Params { must, ..p }, masks[k]);
            }
            answers.push(row);
        }
        if let Some(m) = p.must
            && hand[m as usize] == 0
        {
            return None;
        }

        // Hand the okeys out: best[u] is the top total with u okeys given to the parts so far.
        let total = jokers.min(2);
        let mut best = [NEG; 3];
        best[0] = 0;
        let mut given = vec![[0u8; 3]; count];
        for k in 0..count {
            let mut next = [NEG; 3];
            for used in 0..=total {
                for j in 0..=used {
                    let Some(plan) = &answers[k][j] else { continue };
                    if best[used - j] > NEG / 2 && best[used - j] + plan.score > next[used] {
                        next[used] = best[used - j] + plan.score;
                        given[k][used] = j as u8;
                    }
                }
            }
            best = next;
        }
        if best[total] <= NEG / 2 {
            return None;
        }

        let mut pick = [0usize; MAX_PARTS];
        let mut used = total;
        for k in (0..count).rev() {
            pick[k] = given[k][used] as usize;
            used -= pick[k];
        }
        let mut melds: Vec<(Tile, Shape)> = Vec::new();
        let mut plan = Plan {
            score: best[total] + loose.score,
            ..loose
        };
        for k in 0..count {
            let part = answers[k][pick[k]].as_ref().expect("chosen answer exists");
            melds.extend(
                part.melds
                    .iter()
                    .map(|m| (m.list().iter().copied().filter(|&t| t != JOKER).min().unwrap_or(JOKER), *m)),
            );
            plan.layoffs.extend(&part.layoffs);
            plan.rest.extend(&part.rest);
        }
        // Same order a single search reports: by the lowest tile of each meld, loose tiles ascending.
        // The sort is stable, so two melds starting at the same tile keep their order.
        melds.sort_by_key(|&(anchor, _)| anchor);
        plan.melds = melds.into_iter().map(|(_, m)| m).collect();
        plan.layoffs.sort_unstable();
        plan.rest.sort_unstable();
        Some(plan)
    }

    fn search(&mut self, hand: &Counts, key: u128, p: Params, attach: Attach) -> Option<Plan> {
        self.memo.clear();
        self.counts = *hand;
        self.key = key;
        self.p = p;
        self.attach = attach;
        self.flag = p.must.is_none();
        self.dumps = 0;
        let score = self.solve();
        if score <= NEG / 2 {
            return None;
        }
        let mut plan = Plan {
            score,
            ..Default::default()
        };
        loop {
            let k = self.full_key();
            let (_, ch) = *self.memo.get(&k).expect("memo entry on best path");
            match ch {
                Choice::Leaf => break,
                Choice::Dead(t) => {
                    self.take(t);
                    plan.rest.push(t);
                }
                Choice::Layoff(t) => {
                    self.take(t);
                    self.mark_use(t);
                    plan.layoffs.push(t);
                }
                Choice::Meld(s) => {
                    for &t in s.list().iter() {
                        self.take(t);
                    }
                    self.mark_shape(&s);
                    if s.is_pair() {
                        self.dumps += 1;
                    }
                    plan.melds.push(s);
                }
            }
        }
        for _ in 0..self.counts[JOKER as usize] {
            plan.rest.push(JOKER);
        }
        if self.attach.fits(JOKER) && self.counts[JOKER as usize] > 0 {
            plan.rest.retain(|&t| t != JOKER);
            for _ in 0..self.counts[JOKER as usize] {
                plan.layoffs.push(JOKER);
            }
        }
        Some(plan)
    }

    #[inline]
    fn full_key(&self) -> u128 {
        self.key | ((self.flag as u128) << FLAG_BIT) | ((self.dumps as u128) << DUMP_SHIFT)
    }

    #[inline]
    fn take(&mut self, t: Tile) {
        self.counts[t as usize] -= 1;
        self.key -= 1u128 << (2 * t as u32);
    }

    #[inline]
    fn give(&mut self, t: Tile) {
        self.counts[t as usize] += 1;
        self.key += 1u128 << (2 * t as u32);
    }

    #[inline]
    fn mark_use(&mut self, t: Tile) {
        if self.p.must == Some(t) {
            self.flag = true;
        }
    }

    fn mark_shape(&mut self, s: &Shape) {
        if let Some(m) = self.p.must
            && ((m == JOKER && s.jokers() > 0) || (m != JOKER && s.uses(m)))
        {
            self.flag = true;
        }
    }

    #[inline]
    fn tile_score(&self, t: Tile) -> i32 {
        penalty_of(t, &self.rules) + self.p.cover_w
    }

    fn shape_score(&self, s: &Shape) -> i32 {
        let base = match self.p.mode {
            Mode::Open => s.open_value(&self.rules),
            Mode::Penalty => s.list().iter().map(|&t| penalty_of(t, &self.rules)).sum(),
        };
        base + self.p.cover_w * s.len() as i32
    }

    fn try_shape(&mut self, s: Shape, best: &mut (i32, Choice)) {
        let tiles = s.list();
        for &t in tiles.iter() {
            self.take(t);
        }
        let saved = self.flag;
        self.mark_shape(&s);
        let pair = s.is_pair();
        self.dumps += pair as u8;
        let sub = self.solve();
        self.dumps -= pair as u8;
        self.flag = saved;
        for &t in tiles.iter() {
            self.give(t);
        }
        if sub > NEG / 2 {
            let v = sub + self.shape_score(&s);
            if v > best.0 {
                *best = (v, Choice::Meld(s));
            }
        }
    }

    fn solve(&mut self) -> i32 {
        if !self.flag
            && let Some(m) = self.p.must
            && self.counts[m as usize] == 0
        {
            return NEG;
        }
        let key = self.full_key();
        if let Some(&(s, _)) = self.memo.get(&key) {
            return s;
        }
        let naturals = self.key & NATURALS;
        let first = (naturals != 0).then(|| (naturals.trailing_zeros() / 2) as usize);
        let Some(i) = first else {
            let j = self.counts[JOKER as usize];
            let v = if j > 0 && self.attach.fits(JOKER) {
                j as i32 * self.tile_score(JOKER)
            } else if self.flag {
                0
            } else {
                NEG
            };
            self.memo.insert(key, (v, Choice::Leaf));
            return v;
        };
        let t = i as Tile;
        let mut best = (NEG, Choice::Leaf);

        let may_dead = !(self.p.must == Some(t) && !self.flag && self.counts[i] == 1);
        if may_dead {
            self.take(t);
            let sub = self.solve();
            self.give(t);
            if sub > best.0 {
                best = (sub, Choice::Dead(t));
            }
        }

        if self.attach.fits(t) {
            self.take(t);
            let saved = self.flag;
            self.mark_use(t);
            let sub = self.solve();
            self.flag = saved;
            self.give(t);
            if sub > NEG / 2 {
                let v = sub + self.tile_score(t);
                if v > best.0 {
                    best = (v, Choice::Layoff(t));
                }
            }
        }

        if self.p.pair_dump && self.dumps < self.p.pair_cap {
            let j = self.counts[JOKER as usize];
            if self.counts[i] >= 2 {
                self.try_shape(Shape::Pair { tile: t, joker: false }, &mut best);
            }
            if j >= 1 && self.rules.joker_pairs_with_single {
                self.try_shape(Shape::Pair { tile: t, joker: true }, &mut best);
            }
        }

        if self.p.allow_melds {
            self.enum_groups(t, &mut best);
            self.enum_runs(t, &mut best);
        }

        self.memo.insert(key, best);
        best.0
    }

    fn enum_groups(&mut self, t: Tile, best: &mut (i32, Choice)) {
        let c = color(t);
        let n = num(t);
        let j = self.counts[JOKER as usize];
        let min_len = self.rules.meld_min_len.max(2);
        let mut others = [0u8; 3];
        let mut k = 0;
        for c2 in (c + 1)..NCOLORS {
            if self.counts[id(c2, n) as usize] > 0 {
                others[k] = c2;
                k += 1;
            }
        }
        for mask in 0u8..(1 << k) {
            let mut colors = 1u8 << c;
            for b in 0..k {
                if mask >> b & 1 == 1 {
                    colors |= 1 << others[b];
                }
            }
            let nat = colors.count_ones() as u8;
            for jk in 0..=j.min(4 - nat) {
                let len = nat + jk;
                if len >= min_len && len <= 4 {
                    self.try_shape(
                        Shape::Group {
                            num: n,
                            colors,
                            jokers: jk,
                        },
                        best,
                    );
                }
            }
        }
    }

    fn enum_runs(&mut self, t: Tile, best: &mut (i32, Choice)) {
        let c = color(t);
        let n = num(t);
        // The candidates sit on a shared stack because trying one recurses back into here.
        let start = self.scratch.len();
        self.runs_at(t, c, n);
        if n == 1 && self.rules.one_after_thirteen {
            self.runs_at(t, c, 14);
        }
        for k in start..self.scratch.len() {
            let s = self.scratch[k];
            self.try_shape(s, best);
        }
        self.scratch.truncate(start);
    }

    fn avail(&self, anchor_tile: Tile, c: u8, p: u8) -> bool {
        let tp = pos_tile(c, p);
        tp > anchor_tile && self.counts[tp as usize] > 0
    }

    /// Runs of color `c` through anchor position `a` (tile `t`). Lower-indexed tiles are already
    /// decided, so those positions can only be okeys; an okey never replaces an available natural.
    fn runs_at(&mut self, t: Tile, c: u8, a: u8) {
        let j = if self.p.joker_runs { self.counts[JOKER as usize] as i32 } else { 0 };
        let min_len = self.rules.meld_min_len.max(2);
        let top = if self.rules.one_after_thirteen { 14 } else { 13 };
        let lo_min = if a == 14 { 2 } else { 1 };
        let mut need_left = 0i32;
        let mut lo = a;
        loop {
            if lo < a && !self.avail(t, c, lo) {
                need_left += 1;
            }
            if need_left > j || a - lo + 1 > self.rules.max_run_len.max(min_len) {
                break;
            }
            let hi_max = (if lo == 1 { 13 } else { top }).min(lo + self.rules.max_run_len.max(min_len) - 1);
            let mut need = need_left;
            let mut hi = a;
            loop {
                if hi > a && !self.avail(t, c, hi) {
                    need += 1;
                }
                if need > j {
                    break;
                }
                if hi - lo + 1 >= min_len {
                    let mut mask = 0u16;
                    for p in lo..=hi {
                        if p != a && !self.avail(t, c, p) {
                            mask |= 1 << p;
                        }
                    }
                    self.scratch.push(Shape::Run {
                        color: c,
                        lo,
                        hi,
                        jokers: mask,
                    });
                }
                if hi >= hi_max {
                    break;
                }
                hi += 1;
            }
            if lo <= lo_min {
                break;
            }
            lo -= 1;
        }
    }
}
