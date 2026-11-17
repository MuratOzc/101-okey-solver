use serde::{Deserialize, Serialize};

/// Which arrangement to open with when several reach the threshold.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum OpenStyle {
    /// Highest opening total.
    MaxValue,
    /// Fewest tiles left in hand.
    MaxCover,
    /// Just enough melds to reach the threshold; the rest stay hidden in hand.
    Minimal,
    /// Okey never placed in a run, where anyone holding that tile takes it and I pay for losing it.
    SafeOkey,
    /// Okey kept in hand.
    NoOkey,
}

/// What an opened player lays down besides the opening itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum LayStyle {
    /// Every meld, pair, lay-off and okey swap available.
    All,
    /// The same, but an okey never goes into a run or onto a run's end.
    AllSafe,
    Nothing,
}

/// Tunable heuristics for the simulated players.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    /// Discard: value of holding a neighbour (n±1), a gapped neighbour (n±2), a same-number tile.
    pub w_adj: f32,
    pub w_gap: f32,
    pub w_group: f32,
    /// Discard: weight of partner tiles still unseen, relative to ones already held.
    pub w_live: f32,
    /// Discard: bonus for tiles already inside the best opening arrangement.
    pub w_meld: f32,
    /// Discard: keep high tiles before opening (they count toward the threshold)…
    pub w_value_unopened: f32,
    /// …and shed them after opening (they count against you).
    pub w_value_opened: f32,
    /// Discard: reluctance to throw a tile the next, unopened player is likely to want.
    pub w_danger: f32,
    /// Hold a ready opening while the deck has more tiles than this (nobody else opened / somebody opened).
    pub hold_deck_quiet: i32,
    pub hold_deck_active: i32,
    pub open_style: OpenStyle,
    pub lay_after_open: bool,
    /// After opening, may an okey be laid in a run, where an opponent holding that tile takes it?
    pub joker_in_runs: bool,
    /// Discard: keep pairs in hand to lay onto someone's pairs later.
    pub w_pair_keep: f32,
}

impl Default for Policy {
    fn default() -> Self {
        Policy::BASIC
    }
}

impl Policy {
    /// Champion of self-play tuning (`examples/tune.rs`): holds a ready opening while the deck is
    /// still deep, keeps okeys out of runs when opening, and values high tiles more before opening.
    pub const TUNED: Policy = Policy {
        w_adj: 2.09,
        w_gap: 2.0,
        w_group: 3.0,
        w_live: 0.0165,
        w_meld: 500.0,
        w_value_unopened: 3.38,
        w_value_opened: 3.0,
        w_danger: 0.0,
        hold_deck_quiet: 8,
        hold_deck_active: 10,
        open_style: OpenStyle::SafeOkey,
        lay_after_open: true,
        joker_in_runs: true,
        w_pair_keep: 0.0,
    };

    /// Plays for the finish, not for points: opens as soon as it can, with as few tiles left as
    /// possible, and keeps pairs for whoever opens with pairs.
    pub const FINISHER: Policy = Policy {
        hold_deck_quiet: 99,
        hold_deck_active: 99,
        open_style: OpenStyle::MaxCover,
        w_pair_keep: 60.0,
        ..Policy::TUNED
    };

    /// Like `FINISHER`, but sits on a ready opening until the last round of the deck, as the user's
    /// table does. The search assumes it of every opponent.
    pub const HOLDER: Policy = Policy {
        hold_deck_quiet: 4,
        hold_deck_active: 4,
        ..Policy::FINISHER
    };

    pub fn varied(rng: &mut crate::rng::Rng) -> Policy {
        let mut pol = [Policy::BASIC, Policy::TUNED, Policy::FINISHER][rng.below(3)];
        let mut jitter = |x: f32| x * (0.6 + 0.8 * (rng.next_u64() >> 40) as f32 / (1u64 << 24) as f32);
        pol.w_adj = jitter(pol.w_adj);
        pol.w_gap = jitter(pol.w_gap);
        pol.w_group = jitter(pol.w_group);
        pol.w_value_unopened = jitter(pol.w_value_unopened);
        pol.w_value_opened = jitter(pol.w_value_opened);
        pol.w_live = [0.0, 0.0165, 0.05][rng.below(3)];
        pol.w_danger = [0.0, 0.0, 0.5, 1.5][rng.below(4)];
        if rng.below(2) == 0 {
            pol.hold_deck_quiet = [99, 99, 30, 20, 14, 10, 8, 5][rng.below(8)];
            pol.hold_deck_active = pol.hold_deck_quiet + [0, 2, 6][rng.below(3)];
        }
        if rng.below(2) == 0 {
            pol.open_style = [
                OpenStyle::MaxValue,
                OpenStyle::MaxCover,
                OpenStyle::Minimal,
                OpenStyle::SafeOkey,
                OpenStyle::NoOkey,
            ][rng.below(5)];
        }
        pol
    }

    /// The original hand-written bot: open as soon as possible, discard the least connected tile.
    pub const BASIC: Policy = Policy {
        w_adj: 4.0,
        w_gap: 2.0,
        w_group: 3.0,
        w_live: 0.0,
        w_meld: 500.0,
        w_value_unopened: 1.0,
        w_value_opened: 3.0,
        w_danger: 0.0,
        hold_deck_quiet: 99,
        hold_deck_active: 99,
        open_style: OpenStyle::MaxValue,
        lay_after_open: true,
        joker_in_runs: true,
        w_pair_keep: 0.0,
    };
}
