//! Nothing here may read another player's hand or the order of the deck.

use crate::game::{Game, Opened};
use crate::meld::{Solver, hand_penalty};
use crate::tile::*;

// Bump when the meaning or order of any input changes: data files and networks carry it.
pub const LAYOUT: u32 = 3;

const PLANES: usize = 13;
const LEN_STEPS: [u32; 12] = [1, 2, 3, 4, 5, 6, 8, 10, 12, 14, 17, 20];
const DECK_STEPS: [u32; 16] = [0, 1, 2, 3, 4, 6, 8, 10, 12, 15, 18, 22, 26, 30, 35, 40];
const VALUE_STEPS: [i32; 9] = [-60, -40, -25, -15, -8, -3, 0, 10, 25];
const PAIR_STEPS: [i32; 4] = [-3, -2, -1, 0];
const LOOSE_STEPS: [usize; 10] = [0, 1, 2, 3, 4, 5, 6, 8, 10, 13];
const NFLAGS: usize =
    4 * (3 + LEN_STEPS.len()) + DECK_STEPS.len() + 5 + VALUE_STEPS.len() + PAIR_STEPS.len() + LOOSE_STEPS.len() + 4 + 3 + 4;

pub const NBITS_PLAIN: usize = PLANES * NIDS + NFLAGS;
pub const NBITS_V1: usize = NBITS_PLAIN - 4;
pub const NBITS: usize = NBITS_PLAIN + 2 * NIDS;
pub const WORDS: usize = NBITS.div_ceil(64);
pub const NREAL_PLAIN: usize = 23;
pub const NREAL: usize = NREAL_PLAIN + 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Moment {
    After,
    Draw,
    // The tile must already be in the hand.
    Take(Tile),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feat {
    pub bits: [u64; WORDS],
    pub real: [u8; NREAL],
}

struct Enc {
    f: Feat,
    flag: usize,
    real: usize,
}

impl Enc {
    fn plane(&mut self, k: usize, t: usize) {
        let i = k * NIDS + t;
        self.f.bits[i / 64] |= 1 << (i % 64);
    }

    fn flag(&mut self, on: bool) {
        if on {
            self.f.bits[self.flag / 64] |= 1 << (self.flag % 64);
        }
        self.flag += 1;
    }

    fn draw(&mut self, k: usize, t: usize) {
        let i = NBITS_PLAIN + k * NIDS + t;
        self.f.bits[i / 64] |= 1 << (i % 64);
    }

    fn real(&mut self, x: f32) {
        self.f.real[self.real] = (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        self.real += 1;
    }
}

pub fn encode(g: &Game, sv: &mut Solver, me: u8, moment: Moment, full: bool) -> Feat {
    let mut e = Enc {
        f: Feat {
            bits: [0; WORDS],
            real: [0; NREAL],
        },
        flag: PLANES * NIDS,
        real: 0,
    };
    let seat = |r: usize| (me as usize + r) % 4;
    let my = &g.players[me as usize];
    let hand = &my.hand;
    let unseen = |t: usize| 2u8.saturating_sub(g.seen[t] + hand[t]);
    let offer = match moment {
        Moment::Take(t) => Some(t),
        _ => g.top,
    };
    let attach = g.table.attach(&g.rules);

    for t in 0..NIDS {
        if hand[t] >= 1 {
            e.plane(0, t);
        }
        if hand[t] >= 2 {
            e.plane(1, t);
        }
        if unseen(t) >= 1 {
            e.plane(2, t);
        }
        if unseen(t) >= 2 {
            e.plane(3, t);
        }
        for r in 0..4 {
            if g.piles[seat(r)][t] > 0 {
                e.plane(4 + r, t);
            }
        }
        if attach.tile[t] {
            e.plane(9, t);
        }
    }
    if let Some(t) = offer {
        e.plane(8, t as usize);
    }
    for r in 1..4 {
        if let Some(t) = g.last[seat(r)] {
            e.plane(9 + r, t as usize);
        }
    }

    for r in 0..4 {
        let pl = &g.players[seat(r)];
        e.flag(pl.opened == Opened::No);
        e.flag(pl.opened == Opened::Melds);
        e.flag(pl.opened == Opened::Pairs);
        let n = total(&pl.hand);
        for step in LEN_STEPS {
            e.flag(n <= step);
        }
    }
    let deck = g.deck.len() as u32;
    for step in DECK_STEPS {
        e.flag(deck <= step);
    }
    e.flag(hand[JOKER as usize] >= 1);
    e.flag(hand[JOKER as usize] >= 2);
    e.flag(my.has_indicator);
    e.flag(moment != Moment::After);
    e.flag(matches!(moment, Moment::Take(_)));

    let (value, pairs, loose, loose_points) = g.hand_shape(sv, me);
    let unopened = my.opened == Opened::No;
    for step in VALUE_STEPS {
        e.flag(unopened && value - g.meld_req >= step);
    }
    for step in PAIR_STEPS {
        e.flag(unopened && pairs as i32 - g.pair_req as i32 >= step);
    }
    for step in LOOSE_STEPS {
        e.flag(loose <= step);
    }

    let (mut okeys, mut melds, mut laid) = ([0u8; 4], [0u8; 4], 0u32);
    for m in &g.table.melds {
        let r = ((m.owner + 4 - me) % 4) as usize;
        for by in m.okey_layers() {
            okeys[((by + 4 - me) % 4) as usize] += 1;
        }
        melds[r] += 1;
        laid += m.shape.len() as u32;
    }
    for r in 0..4 {
        e.flag(okeys[r] > 0);
    }

    let mut d = [0f32; 7];
    let (mut fits, mut right_threw) = (false, false);
    let thrown = offer.filter(|_| moment == Moment::After);
    if let Some(t) = thrown {
        fits = attach.fits(t);
        if t != JOKER {
            let right = &g.piles[seat(1)];
            let (c, n) = (color(t), num(t) as i32);
            right_threw = right[t as usize] > 0;
            d[0] = unseen(t as usize) as f32;
            for (k, dist) in [(1, 1), (2, 2)] {
                for nn in [n - dist, n + dist] {
                    if (1..=13).contains(&nn) {
                        let u = id(c, nn as u8) as usize;
                        d[k] += unseen(u) as f32 / 4.0;
                        d[5] += right[u] as f32 / 4.0;
                    }
                }
            }
            for c2 in (0..NCOLORS).filter(|&c2| c2 != c) {
                let u = id(c2, n as u8) as usize;
                d[3] += unseen(u).min(1) as f32 / 3.0;
                d[6] += right[u].min(1) as f32 / 3.0;
            }
            d[4] = n as f32 / 13.0;
        }
    }
    e.flag(fits);
    e.flag(right_threw);
    e.flag(thrown == Some(JOKER));
    let next = if moment == Moment::Draw { 0 } else { 1 };
    for r in 0..4 {
        e.flag(deck > 0 && (next + deck - 1) % 4 == r);
    }

    e.real(deck as f32 / 60.0);
    for r in 0..4 {
        e.real(total(&g.players[seat(r)].hand) as f32 / 22.0);
    }
    e.real(hand_penalty(hand, &g.rules) as f32 / 250.0);
    e.real(if unopened { value as f32 / 200.0 } else { 0.0 });
    e.real(if unopened { pairs as f32 / 8.0 } else { 0.0 });
    e.real(loose as f32 / 22.0);
    e.real(loose_points as f32 / 250.0);
    for r in 0..4 {
        e.real(melds[r] as f32 / 8.0);
    }
    e.real(laid as f32 / 60.0);
    e.real(total(&g.piles[me as usize]) as f32 / 20.0);
    for x in d {
        e.real(x);
    }
    debug_assert_eq!((e.flag, e.real), (NBITS_PLAIN, NREAL_PLAIN));
    if full {
        let (mut all, mut big, mut useful) = (0u32, 0u32, 0u32);
        let outs = g.outs(sv, me);
        for t in 0..NIDS {
            let n = unseen(t) as u32;
            all += n;
            if outs[t].0 {
                e.draw(0, t);
                big += n;
            }
            if outs[t].1 {
                e.draw(1, t);
                useful += n;
            }
        }
        e.real(big as f32 / all.max(1) as f32);
        e.real(useful as f32 / all.max(1) as f32);
        e.real(all as f32 / 106.0);
    }
    e.f
}
