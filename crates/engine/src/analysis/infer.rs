//! Dealing the hidden tiles so that they fit what the opponents have done.

use super::*;

/// How much less likely a deal is when an opponent "would have" kept a tile they threw away…
pub(super) const DISCARD_FITS: f32 = 0.35;
/// …or is sitting on an opening they would normally have laid down.
pub(super) const UNOPENED_BUT_READY: f32 = 0.2;
pub(super) const INFER_TRIES: usize = 12;
/// A player's throwing style counts once this many of their discards are seen…
pub(super) const STYLE_AFTER: usize = 2;
/// …and picks among this many of the tiles their hand needs least.
pub(super) const STYLE_CHOICES: usize = 3;
pub(super) const DISCARDS_REMEMBERED: usize = 4;
/// Whether `d` would complete a run or a group with two tiles of `hand`.
pub(super) fn completes(hand: &Counts, d: Tile) -> bool {
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
        self.plausibility_with(g, sv, None)
    }

    /// `ready`: how likely an unopened player is to sit on an opening they could lay; None uses the
    /// search's fixed rule (held back only while the bots' policy would hold).
    pub(crate) fn plausibility_with(&self, g: &Game, sv: &mut Solver, ready: Option<f32>) -> f32 {
        let mut w = 1.0f32;
        for o in 1..4usize {
            let pv = &self.players[o];
            let hand = &g.players[o].hand;
            for &d in pv.discards.iter().rev().take(DISCARDS_REMEMBERED) {
                if completes(hand, d) {
                    w *= DISCARD_FITS;
                }
            }
            let factor = ready.or_else(|| (!g.should_hold(o as u8)).then_some(UNOPENED_BUT_READY));
            if let Some(f) = factor
                && pv.opened == Opened::No
                && !pv.discards.is_empty()
            {
                let can = g.open_search(sv, hand, None).is_some_and(|p| p.open_value(&g.rules) >= g.meld_req)
                    || g.pairs_ok(o as u8, None).is_some();
                if can {
                    w *= f;
                }
            }
        }
        w
    }

    /// A deal of the hidden tiles, optionally drawn in proportion to how plausible it is.
    pub fn sample(&self, rng: &mut Rng, sv: &mut Solver, opts: &Options) -> Game {
        let deal = |rng: &mut Rng, sv: &mut Solver| {
            let mut g = self.determinize(rng);
            g.policy = [opts.policy; 4];
            if opts.build {
                self.build_hands(&mut g, sv, rng);
            }
            g
        };
        let mut g = deal(rng, sv);
        if opts.infer {
            for _ in 0..INFER_TRIES {
                let w = self.plausibility(&g, sv);
                if (rng.below(10_000) as f32) < w * 10_000.0 {
                    break;
                }
                g = deal(rng, sv);
            }
        }
        g
    }

    /// Draws each player has made so far, about: the deck is shared out evenly between the seats.
    fn draws_so_far(&self) -> usize {
        let dealt = self.rules.hand_size as usize * 4 + self.rules.starter_extra as usize;
        let first_deck = 106usize.saturating_sub(dealt + 1);
        first_deck.saturating_sub(self.deck_count as usize).div_ceil(4)
    }

    /// A random deal gives an unopened opponent a hand as loose as the deck, but a real one has been
    /// built: every turn the player kept what fitted and threw what did not. Replay half of those turns
    /// with tiles that could have come instead, from the deck or another opponent's unknown tiles.
    /// Among the tiles the hand needs least, the one thrown is the one closest in number to what that
    /// player has been throwing, so a player who throws low tiles ends up holding high ones.
    pub(crate) fn build_hands(&self, g: &mut Game, sv: &mut Solver, rng: &mut Rng) {
        let known: Vec<Counts> = self.players.iter().map(|p| counts_from(&p.known)).collect();
        let free = |g: &Game, q: usize, t: usize| g.players[q].hand[t].saturating_sub(known[q][t]);
        let style: Vec<Option<f32>> = self
            .players
            .iter()
            .map(|p| {
                let nums: Vec<f32> = p.discards.iter().filter(|&&t| t != JOKER).map(|&t| num(t) as f32).collect();
                (nums.len() >= STYLE_AFTER).then(|| nums.iter().sum::<f32>() / nums.len() as f32)
            })
            .collect();
        // Half the turns: replaying all of them made openings likelier than in bot play (50% against 47%).
        for _ in 0..self.draws_so_far().div_ceil(2) {
            for o in 1..4usize {
                if g.players[o].opened != Opened::No {
                    continue;
                }
                let mut src: Vec<(usize, Tile)> = g.deck.iter().map(|&t| (0, t)).collect();
                for q in (1..4).filter(|&q| q != o) {
                    for t in 0..NIDS {
                        for _ in 0..free(g, q, t) {
                            src.push((q, t as Tile));
                        }
                    }
                }
                if src.is_empty() {
                    return;
                }
                let (from, t) = src[rng.below(src.len())];
                g.players[o].hand[t as usize] += 1;
                let mut ranked: Vec<(Tile, i32)> = g
                    .discard_scores(sv, o as u8)
                    .into_iter()
                    .filter(|&(d, _)| d != JOKER && free(g, o, d as usize) > 0)
                    .collect();
                ranked.sort_by_key(|&(_, keep)| keep);
                let pick = match style[o] {
                    Some(m) => ranked
                        .iter()
                        .take(STYLE_CHOICES)
                        .min_by(|a, b| (num(a.0) as f32 - m).abs().total_cmp(&(num(b.0) as f32 - m).abs())),
                    None => ranked.first(),
                };
                let Some(&(d, _)) = pick else {
                    g.players[o].hand[t as usize] -= 1;
                    continue;
                };
                g.players[o].hand[d as usize] -= 1;
                if d == t {
                    continue;
                }
                if from == 0 {
                    let i = g.deck.iter().position(|&x| x == t).expect("drawn from the deck");
                    g.deck[i] = d;
                } else {
                    g.players[from].hand[t as usize] -= 1;
                    g.players[from].hand[d as usize] += 1;
                }
            }
        }
    }
}
