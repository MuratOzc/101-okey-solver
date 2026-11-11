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
    };

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
    };
}
