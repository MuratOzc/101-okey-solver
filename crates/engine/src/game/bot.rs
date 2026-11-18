//! How the hand-written bots draw, lay and throw.

use super::*;

pub(super) fn conn(hand: &Counts, t: Tile) -> i32 {
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

impl Game {
    /// Whether the bot keeps a ready opening in hand for now.
    pub fn should_hold(&self, p: u8) -> bool {
        let pol = &self.policy[p as usize];
        let active = (0..4u8).any(|q| q != p && self.players[q as usize].opened != Opened::No);
        self.deck.len() as i32 > if active { pol.hold_deck_active } else { pol.hold_deck_quiet }
    }

    pub(super) fn live_copies(&self, p: u8, t: Tile) -> i32 {
        2 - self.seen[t as usize] as i32 - self.players[p as usize].hand[t as usize] as i32
    }

    pub(super) fn pursue_pairs(&self, sv: &mut Solver, p: u8) -> bool {
        let pl = &self.players[p as usize];
        if !self.rules.pairs_enabled || pl.opened != Opened::No {
            return false;
        }
        if (pair_stats(&pl.hand, &self.rules, self.indicator_for(p)).0 as i32) < self.pair_req as i32 - 1 {
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
        if pl.opened == Opened::No && self.policy[p as usize].hold_side && self.should_hold(p) {
            return false;
        }
        self.can_use_taken(sv, p, t)
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
            if c == OpenChoice::Finish && !self.in_one_possible(sv, p, taken) {
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
                    self.discard_or_bot(sv, p, None);
                    return None;
                }
                Err(()) => {
                    if let Some(b) = backup {
                        *self = b;
                    }
                }
            }
        }
        self.discard_or_bot(sv, p, None);
        None
    }

    /// Begin `p`'s turn the way a bot does: take the side tile if it wants it, else draw.
    /// Gives the tile taken, or the end of the hand when the deck is empty.
    pub(super) fn bot_draw(&mut self, sv: &mut Solver, p: u8) -> Result<Option<Tile>, Outcome> {
        if self.deck.is_empty() {
            return Err(Outcome::DeckOut);
        }
        self.table.new_turn();
        if self.bot_wants_take(sv, p) {
            return Ok(Some(self.take_top(p)));
        }
        self.draw(p);
        Ok(None)
    }

    /// One full bot turn for the current player.
    pub fn bot_turn(&mut self, sv: &mut Solver) -> Option<Outcome> {
        let p = self.current;
        let taken = match self.bot_draw(sv, p) {
            Ok(taken) => taken,
            Err(end) => return Some(end),
        };
        let res = self.bot_act(sv, p, taken);
        if res.is_none() {
            self.current = (p + 1) % 4;
        }
        res
    }
}
