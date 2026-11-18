use crate::game::Opened;
use crate::tile::{JOKER, Tile, num};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    pub partners: bool,
    pub hand_size: u8,
    pub starter_extra: u8,

    pub open_min: i32,
    pub pairs_enabled: bool,
    pub pair_min: u8,
    #[serde(alias = "katlamali")]
    pub raising: bool,
    pub raise_meld_by: i32,
    pub raise_pair_by: u8,

    pub meld_min_len: u8,
    pub max_run_len: u8,
    pub layoff_per_end: u8,
    pub pairs_per_turn: u8,
    #[serde(alias = "ace_after_king")]
    pub one_after_thirteen: bool,
    #[serde(alias = "ace_high_value")]
    pub high_one_value: i32,

    pub joker_pairs_with_single: bool,
    pub two_jokers_pair: bool,
    #[serde(alias = "gosterge_pair")]
    pub indicator_pair: bool,

    pub take_must_use: bool,
    pub take_penalty_enabled: bool,
    pub take_penalty_mult: i32,

    pub new_melds_after_open: bool,
    pub pair_opener_can_layoff: bool,
    pub pair_opener_can_meld: bool,
    pub pairs_onto_pair_opener: bool,

    pub joker_retrieve: bool,
    pub group_joker_must_complete: bool,
    pub okey_lost_penalty: i32,

    pub score_finish: i32,
    pub score_unopened: i32,
    pub pair_hand_mult: i32,
    pub joker_in_hand: i32,
    pub mult_okey_finish: i32,
    #[serde(alias = "mult_elden")]
    pub mult_in_one: i32,
    pub mult_pair_finish: i32,
    #[serde(alias = "elden_requires_open_min")]
    pub in_one_requires_open_min: bool,
    #[serde(alias = "open_finish_is_elden")]
    pub open_finish_is_in_one: bool,

    #[serde(alias = "penalty_islek")]
    pub penalty_playable: i32,
    #[serde(alias = "islek_applies_unopened")]
    pub playable_applies_unopened: bool,
    pub penalty_discard_joker: i32,

    pub big_pairs_penalty: i32,
    pub big_pairs_min: u8,
}

impl Rules {
    /// What taking `t` from the side costs the taker.
    pub fn take_cost(&self, t: Tile) -> i32 {
        if !self.take_penalty_enabled || t == JOKER {
            return 0;
        }
        num(t) as i32 * self.take_penalty_mult
    }

    pub fn same_team(&self, a: u8, b: u8) -> bool {
        a == b || (self.partners && a == (b + 2) % 4)
    }

    /// What opening needs once the given openings (kind, points or pairs) are down: (meld points, pairs).
    pub fn thresholds(&self, openings: impl IntoIterator<Item = (Opened, i32)>) -> (i32, u8) {
        let (mut meld, mut pair) = (self.open_min, self.pair_min);
        if self.raising {
            for (kind, value) in openings {
                match kind {
                    Opened::Melds => meld = meld.max(value + self.raise_meld_by),
                    Opened::Pairs => pair = pair.max(value as u8 + self.raise_pair_by),
                    Opened::No => {}
                }
            }
        }
        (meld, pair)
    }

    /// What opening with this many pairs takes off the opener's score.
    pub fn big_pairs_reward(&self, pairs: u8) -> i32 {
        if pairs >= self.big_pairs_min { self.big_pairs_penalty } else { 0 }
    }

    /// What every score of the hand is multiplied by after such a finish.
    pub fn finish_mult(&self, okey: bool, in_one: bool, pairs: bool) -> i32 {
        [
            (okey, self.mult_okey_finish),
            (in_one, self.mult_in_one),
            (pairs, self.mult_pair_finish),
        ]
        .into_iter()
        .filter(|&(on, _)| on)
        .map(|(_, m)| m)
        .product()
    }

    /// What a hand that did not finish is charged before that multiplier. `left` is what its tiles
    /// count, okeys included.
    pub fn hand_charge(&self, opened: Opened, left: i32, okeys: i32) -> i32 {
        match opened {
            Opened::No => self.score_unopened + okeys * self.joker_in_hand,
            Opened::Melds => left,
            Opened::Pairs => left * self.pair_hand_mult,
        }
    }
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            partners: false,
            hand_size: 21,
            starter_extra: 1,
            open_min: 101,
            pairs_enabled: true,
            pair_min: 5,
            raising: false,
            raise_meld_by: 1,
            raise_pair_by: 1,
            meld_min_len: 3,
            max_run_len: 5,
            layoff_per_end: 2,
            pairs_per_turn: 3,
            one_after_thirteen: false,
            high_one_value: 1,
            joker_pairs_with_single: true,
            two_jokers_pair: false,
            indicator_pair: true,
            take_must_use: true,
            take_penalty_enabled: false,
            take_penalty_mult: 10,
            new_melds_after_open: true,
            pair_opener_can_layoff: true,
            pair_opener_can_meld: false,
            pairs_onto_pair_opener: true,
            joker_retrieve: true,
            group_joker_must_complete: true,
            okey_lost_penalty: 101,
            score_finish: -101,
            score_unopened: 101,
            pair_hand_mult: 2,
            joker_in_hand: 101,
            mult_okey_finish: 2,
            mult_in_one: 2,
            mult_pair_finish: 2,
            in_one_requires_open_min: false,
            open_finish_is_in_one: false,
            penalty_playable: 101,
            playable_applies_unopened: true,
            penalty_discard_joker: 101,
            big_pairs_penalty: 101,
            big_pairs_min: 7,
        }
    }
}
