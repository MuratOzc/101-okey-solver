use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TakenJokerPenalty {
    None,
    PlacedValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    pub partners: bool,
    pub hand_size: u8,
    pub starter_extra: u8,

    pub open_min: i32,
    pub pairs_enabled: bool,
    pub pair_min: u8,
    pub katlamali: bool,
    pub raise_meld_by: i32,
    pub raise_pair_by: u8,

    pub meld_min_len: u8,
    pub max_run_len: u8,
    pub layoff_per_end: u8,
    pub pairs_per_turn: u8,
    pub ace_after_king: bool,
    pub ace_high_value: i32,

    pub joker_pairs_with_single: bool,
    pub two_jokers_pair: bool,
    pub gosterge_pair: bool,

    pub take_must_use: bool,
    pub take_penalty_enabled: bool,
    pub take_penalty_mult: i32,
    pub take_joker_penalty: TakenJokerPenalty,

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
    pub mult_elden: i32,
    pub mult_pair_finish: i32,
    pub elden_requires_open_min: bool,
    pub open_finish_is_elden: bool,

    pub penalty_islek: i32,
    pub islek_applies_unopened: bool,
    pub penalty_discard_joker: i32,

    pub big_pairs_penalty: i32,
    pub big_pairs_min: u8,
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
            katlamali: false,
            raise_meld_by: 1,
            raise_pair_by: 1,
            meld_min_len: 3,
            max_run_len: 5,
            layoff_per_end: 2,
            pairs_per_turn: 3,
            ace_after_king: false,
            ace_high_value: 1,
            joker_pairs_with_single: true,
            two_jokers_pair: false,
            gosterge_pair: true,
            take_must_use: true,
            take_penalty_enabled: false,
            take_penalty_mult: 10,
            take_joker_penalty: TakenJokerPenalty::None,
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
            mult_elden: 2,
            mult_pair_finish: 2,
            elden_requires_open_min: false,
            open_finish_is_elden: false,
            penalty_islek: 101,
            islek_applies_unopened: true,
            penalty_discard_joker: 101,
            big_pairs_penalty: 101,
            big_pairs_min: 7,
        }
    }
}
