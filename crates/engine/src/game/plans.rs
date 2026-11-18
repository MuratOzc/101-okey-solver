//! The plans a player can follow in a turn, and searching over them by playing each out.

use super::*;

impl Game {
    /// The opening / laying plans worth trying for a player in the given state.
    pub fn plan_choices(opened: Opened) -> Vec<OpenChoice> {
        match opened {
            Opened::No => {
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
            _ => vec![
                OpenChoice::Lay(LayStyle::All),
                OpenChoice::Lay(LayStyle::AllSafe),
                OpenChoice::Lay(LayStyle::Nothing),
            ],
        }
    }

    /// The distinct positions those plans lead `p` to: the plan, the game after it and before any
    /// discard, and how the hand ended if the plan ended it.
    pub fn plan_positions(&self, sv: &mut Solver, p: u8, taken: Option<Tile>) -> Vec<(OpenChoice, Game, Option<Outcome>)> {
        let mut out: Vec<(OpenChoice, Game, Option<Outcome>)> = Vec::new();
        for choice in Self::plan_choices(self.players[p as usize].opened) {
            let mut g = self.clone();
            let Ok(end) = g.apply_choice(sv, p, choice, taken) else { continue };
            let same = |(_, o, o_end): &(OpenChoice, Game, Option<Outcome>)| {
                *o_end == end
                    && o.players[p as usize].hand == g.players[p as usize].hand
                    && o.players.iter().zip(&g.players).all(|(a, b)| a.penalty == b.penalty)
                    && o.table.melds.len() == g.table.melds.len()
                    && o.table
                        .melds
                        .iter()
                        .zip(&g.table.melds)
                        .all(|(a, b)| a.owner == b.owner && a.shape == b.shape)
            };
            if !out.iter().any(same) {
                out.push((choice, g, end));
            }
        }
        out
    }

    /// The `n` tiles the bot would soonest throw, best first.
    pub fn best_discards(&self, sv: &mut Solver, p: u8, n: usize) -> Vec<Tile> {
        let mut scores = self.discard_scores(sv, p);
        scores.sort_by_key(|&(_, keep)| keep);
        scores.into_iter().take(n).map(|(t, _)| t).collect()
    }

    /// (best opening total, pairs, tiles left loose after laying everything, their points)
    pub fn hand_shape(&self, sv: &mut Solver, p: u8) -> (i32, usize, usize, i32) {
        let pl = &self.players[p as usize];
        let hand = pl.hand;
        let (value, pairs, plan) = match pl.opened {
            Opened::No => {
                let value = self.open_search(sv, &hand, None).map_or(0, |plan| plan.open_value(&self.rules));
                let pairs = pair_stats(&hand, &self.rules, self.gosterge_for(p)).0;
                (value, pairs, sv.run(&hand, Params::cover(None), None))
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
    pub(super) fn lookahead_turn(&mut self, sv: &mut Solver, hero: u8, deeper: u8) -> Option<Outcome> {
        match self.bot_draw(sv, hero) {
            Ok(taken) => self.lookahead_act(sv, hero, taken, deeper),
            Err(end) => Some(end),
        }
    }

    /// `hero` has drawn: pick the plan and discard whose played-out continuation is best, and make it.
    pub fn lookahead_act(&mut self, sv: &mut Solver, hero: u8, taken: Option<Tile>, deeper: u8) -> Option<Outcome> {
        let mut best: Option<(i32, Game, Option<Outcome>)> = None;
        for (_, after, res) in self.plan_positions(sv, hero, taken) {
            let lines: Vec<Game> = match res {
                Some(_) => vec![after],
                None => after
                    .best_discards(sv, hero, LOOKAHEAD_DISCARDS)
                    .into_iter()
                    .map(|t| {
                        let mut g = after.clone();
                        g.discard(hero, t);
                        g.current = (hero + 1) % 4;
                        g
                    })
                    .collect(),
            };
            for g in lines {
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
}
