use super::*;
use okey_engine::table::Side;

fn t(s: &str) -> Tile {
    parse(s).unwrap()
}

fn started() -> State {
    let mut s = State::default();
    s.key(t("S7")).unwrap();
    assert_eq!(s.mode, Mode::Starter);
    s.set_starter(0);
    let hand = "K1 K2 K3 K4 K5 M9 M10 M11 S1 S1 Si4 Si5 Si6 K12 M12 S12 Si12 M3 M4 S9 Si13 K13";
    for x in hand.split_whitespace() {
        s.key(t(x)).unwrap();
    }
    s.start().unwrap();
    s
}

fn enter_meld(s: &mut State, tiles: &str) {
    s.mode = Mode::MeldEntry;
    for x in tiles.split_whitespace() {
        s.key(t(x)).unwrap();
    }
    s.lay_meld().unwrap();
    s.mode = Mode::Idle;
}

#[test]
fn setup_and_flow() {
    let mut s = started();
    assert_eq!(s.deck_count, 20);
    assert!(!s.keyboard_visible(), "my own turn needs no keyboard");
    assert!(s.view().is_some_and(|v| v.phase == Phase::AfterDraw));
    s.discard(0, t("Si13")).unwrap();
    assert_eq!(s.turn, 1);
    assert!(!s.keyboard_visible(), "an opponent must draw before discarding");
    s.opp_draw().unwrap();
    assert!(s.keyboard_visible());
    s.key(t("K7")).unwrap();
    s.opp_draw().unwrap();
    s.key(t("M7")).unwrap();
    s.rules.take_must_use = false;
    s.take_side().unwrap();
    assert_eq!(s.players[3].known, vec![t("M7")]);
    s.key(t("Si8")).unwrap();
    s.rules.take_must_use = true;
    assert_eq!(s.players[3].known, vec![t("M7")], "a taken tile stays known until it is shown");
    assert!(s.view().is_some_and(|v| v.phase == Phase::BeforeDraw));
    assert!(!s.can_take(), "Si8 fits no meld of mine, so it cannot be used to open");
    s.apply(Decision::Draw).unwrap();
    assert_eq!(s.mode, Mode::PickDraw);
    s.key(t("K6")).unwrap();
    assert!(s.view().is_some_and(|v| v.phase == Phase::AfterDraw));
}

#[test]
fn taken_tile_must_be_used() {
    let mut s = started();
    s.discard(0, t("Si13")).unwrap();
    for d in ["K7", "M7"] {
        s.opp_draw().unwrap();
        s.key(t(d)).unwrap();
    }
    s.take_side().unwrap();
    assert!(s.discard(3, t("Si8")).is_err(), "M7 was taken and not laid");
    enter_meld(&mut s, "M6 M7 M8");
    enter_meld(&mut s, "K9 K10 K11");
    enter_meld(&mut s, "S9 S10 S11 S12 S13");
    s.discard(3, t("Si8")).unwrap();
}

#[test]
fn seven_pairs_reward() {
    let seven = "K1 K1 M2 M2 Si3 Si3 S5 S5 K8 K8 M9 M9 Si11 Si11";
    let opened = |finishing: bool| {
        let mut s = started();
        s.hand = seven.split_whitespace().map(t).collect();
        s.hand.extend(if finishing {
            vec![t("K13")]
        } else {
            vec![t("K13"), t("M13"), t("S9")]
        });
        s.mode = Mode::Manual;
        for pair in seven.split_whitespace().collect::<Vec<_>>().chunks(2) {
            s.buffer = pair.iter().map(|x| t(x)).collect();
            s.lay_meld().unwrap();
        }
        assert_eq!(s.players[0].open_value, 7);
        s
    };
    let mut by_hand = opened(false);
    by_hand.discard(0, t("K13")).unwrap();
    assert_eq!(by_hand.penalties[0], -101);

    let mut by_advice = opened(false);
    by_advice.mode = Mode::Idle;
    by_advice
        .apply(Decision::Act {
            choice: okey_engine::game::OpenChoice::Stay,
            discard: Some(t("K13")),
        })
        .unwrap();
    assert_eq!(by_advice.penalties[0], -101, "the advised discard must not swallow the reward");

    let mut out = opened(true);
    out.discard(0, t("K13")).unwrap();
    assert_eq!(out.stage, Stage::Over);
    assert_eq!(out.penalties[0], -101, "going out in the same turn keeps the reward");
}

#[test]
fn take_penalty_is_charged() {
    let mut s = started();
    s.rules.take_penalty_enabled = true;
    s.discard(0, t("Si13")).unwrap();
    for d in ["K7", "M7"] {
        s.opp_draw().unwrap();
        s.key(t(d)).unwrap();
    }
    s.take_side().unwrap();
    assert_eq!(s.penalties, [0, 0, 0, 70], "taking a 7 costs 10 × 7");
}

#[test]
fn two_okeys_are_no_pair() {
    let mut s = started();
    s.discard(0, t("Si13")).unwrap();
    s.opp_draw().unwrap();
    s.mode = Mode::MeldEntry;
    s.buffer = vec![JOKER, JOKER];
    assert!(s.lay_meld().is_err());
    s.rules.pairs_enabled = false;
    s.buffer = vec![t("K9"), t("K9")];
    assert!(s.lay_meld().is_err(), "no pairs when the table does not play them");
}

#[test]
fn indicator_not_held_twice() {
    let mut s = State::default();
    s.key(t("S7")).unwrap();
    s.set_starter(1);
    s.key(t("K1")).unwrap();
    s.key(t("K1")).unwrap();
    s.mode = Mode::Indicator;
    assert!(!s.key_enabled(t("K1")));
    assert!(s.key_enabled(t("K2")));
}

#[test]
fn usable_side_tile_taken() {
    let mut s = started();
    s.discard(0, t("Si13")).unwrap();
    for d in ["K7", "M7", "K9"] {
        s.opp_draw().unwrap();
        s.key(t(d)).unwrap();
    }
    assert!(s.can_take(), "K9 makes a group of nines and still opens with 105");
}

#[test]
fn opponent_layoff_options() {
    let mut s = started();
    s.discard(0, t("Si13")).unwrap();
    s.opp_draw().unwrap();
    assert!(s.lay_options(0).is_empty());
    enter_meld(&mut s, "M6 M7 M8");
    enter_meld(&mut s, "K9 K10 K11");
    enter_meld(&mut s, "S9 S10 S11 S12 S13");
    assert_eq!(s.players[1].opened, Opened::Melds);
    assert_eq!(s.players[1].open_value, 106);
    assert_eq!(s.players[1].hand_len, Some(11));
    assert!(s.meld_tappable(0));
    let opts = s.lay_options(0);
    assert!(opts.iter().any(|o| o.tile == t("M5") && o.side == Side::Low));
    assert!(
        opts.iter()
            .any(|o| o.tile == JOKER && o.side == Side::High && o.stands_for == t("M9"))
    );
    let okey_low = *opts.iter().find(|o| o.tile == JOKER && o.side == Side::Low).unwrap();
    s.lay_option(0, okey_low).unwrap();
    assert_eq!(
        s.table.melds[0].shape,
        Shape::Run {
            color: 2,
            lo: 5,
            hi: 8,
            jokers: 1 << 5
        }
    );
    assert_eq!(s.players[1].hand_len, Some(10));
    assert_eq!(s.swap_options(0).len(), 1, "whoever holds M5 could now take the okey back");
}

#[test]
fn indicator_pairs_with_any_tile() {
    let mut s = started();
    s.discard(0, t("Si13")).unwrap();
    s.opp_draw().unwrap();
    for p in ["S7 K7", "K8 K8", "M6 M6", "Si2 Si2", "S4 S4"] {
        enter_meld(&mut s, p);
    }
    assert_eq!(
        s.table.melds[0].shape,
        Shape::IndicatorPair {
            indicator: t("S7"),
            other: t("K7")
        }
    );
    assert_eq!(s.players[1].opened, Opened::Pairs);
    assert_eq!(s.players[1].open_value, 5);
    assert!(s.opening_complete());

    let mut s = started();
    s.hand.retain(|&x| x != t("S1"));
    s.hand.extend([t("S7"), t("K7")]);
    s.mode = Mode::Manual;
    s.buffer = vec![t("S7"), t("K7")];
    assert!(s.buffer_shape().is_none(), "I was not dealt the gösterge");
    s.players[0].has_indicator = true;
    assert!(s.buffer_shape().is_some_and(|sh| sh.is_pair()));
}

#[test]
fn no_table_moves_before_101() {
    let mut s = started();
    s.discard(0, t("Si13")).unwrap();
    s.opp_draw().unwrap();
    enter_meld(&mut s, "M6 M7 M8");
    assert!(!s.opening_complete(), "21 points is not an opening");
    assert_eq!(s.opening_progress().as_deref(), Some("Açılış: 21 / 101 puan"));
    assert!(s.lay_options(0).is_empty(), "no lay-off before 101 is down");
    assert!(s.swap_options(0).is_empty());
    assert!(!s.meld_tappable(0));
    assert!(s.discard(1, t("K7")).is_err(), "the turn cannot end on a half opening");
    enter_meld(&mut s, "K9 K10 K11");
    enter_meld(&mut s, "S9 S10 S11 S12 S13");
    assert!(s.opening_complete(), "21 + 30 + 55 = 106");
    assert!(!s.lay_options(0).is_empty());
    s.discard(1, t("K7")).unwrap();
}

#[test]
fn half_opening_stops_analysis() {
    let mut s = started();
    s.mode = Mode::Manual;
    s.buffer = vec![t("K1"), t("K2"), t("K3")];
    s.lay_meld().unwrap();
    assert!(!s.opening_complete());
    assert!(s.view().is_none(), "no advice may be computed from an illegal half-opened position");
    assert!(s.discard(0, t("S9")).is_err());
}

#[test]
fn raising_beats_previous_opening() {
    let mut s = started();
    s.rules.raising = true;
    s.discard(0, t("Si13")).unwrap();
    s.opp_draw().unwrap();
    enter_meld(&mut s, "K9 K10 K11");
    enter_meld(&mut s, "S9 S10 S11 S12 S13");
    enter_meld(&mut s, "M6 M7 M8");
    assert_eq!(s.players[1].open_value, 106);
    s.discard(1, t("K7")).unwrap();
    s.opp_draw().unwrap();
    assert_eq!(s.opening_need(), (107, 5), "the next opener needs more than 106");
}

#[test]
fn match_stakes_carry_over() {
    let mut s = started();
    assert!(s.view().unwrap().stakes.is_none());
    s.hand_no = 10;
    assert!(
        s.view().unwrap().stakes.is_none(),
        "no scores kept, so nothing to play the match for"
    );
    s.totals = [300, 50, 260, 330];
    let st = s.view().unwrap().stakes.unwrap();
    assert_eq!((st.hands_left, st.behind), (1, [250, 40, -30]));
    s.hand_no = 9;
    assert!(
        s.view().unwrap().stakes.is_none(),
        "three hands from the end the score gap still decides"
    );
    s.hand_no = 10;
    let next = s.new_hand();
    assert_eq!((next.hand_no, next.totals), (11, [300, 50, 260, 330]));
    assert_eq!(next.new_hand().hand_no, 11, "the hand number stops at the last hand");
    s.rules.partners = true;
    assert_eq!(s.view().unwrap().stakes.unwrap().behind, [180; 3]);
}

#[test]
fn disabled_keys() {
    let mut s = started();
    assert!(!s.key_enabled(t("K1")), "no keyboard input on my own action turn");
    s.discard(0, t("Si13")).unwrap();
    s.opp_draw().unwrap();
    assert!(s.key_enabled(t("K7")));
    assert!(!s.key_enabled(t("S1")), "both S1 are in my hand");
}

#[test]
fn hand_ends_after_last_tile() {
    let mut s = started();
    s.discard(0, t("Si13")).unwrap();
    s.deck_count = 1;
    s.opp_draw().unwrap();
    s.key(t("K7")).unwrap();
    assert_eq!(s.stage, Stage::Over, "nobody may take the side tile once the deck is empty");
}

#[test]
fn hand_scored_into_match() {
    let mut s = started();
    s.discard(0, t("Si13")).unwrap();
    s.opp_draw().unwrap();
    enter_meld(&mut s, "M6 M7 M8");
    enter_meld(&mut s, "K9 K10 K11");
    enter_meld(&mut s, "S9 S10 S11 S12 S13");
    s.finish(1, Some(JOKER));
    s.fin_in_one = false;
    s.players[3].opened = Opened::Melds;
    assert_eq!(s.hand_scores(), None, "nothing is scored before the leftovers are entered");
    s.left[3] = Some(40);
    s.okeys_left[2] = 1;
    assert_eq!(
        s.hand_scores(),
        Some([202, -202, 404, 80]),
        "finishing with the okey doubles every hand"
    );
    let next = s.new_hand();
    assert_eq!((next.totals, next.history.len()), ([202, -202, 404, 80], 1));
    assert_eq!(next.lead(), [-404, 202, -122]);
}

#[test]
fn penalties_written_at_once() {
    let mut s = started();
    s.discard(0, t("Si13")).unwrap();
    s.opp_draw().unwrap();
    enter_meld(&mut s, "M6 M7 M8");
    enter_meld(&mut s, "K9 K10 K11");
    enter_meld(&mut s, "S9 S10 S11 S12 S13");
    s.discard(1, t("M9")).unwrap();
    assert_eq!(s.penalties, [0, 101, 0, 0], "M9 fits the run M6-M8 on the table");
    s.opp_draw().unwrap();
    s.discard(2, t("K2")).unwrap();
    assert_eq!(s.penalties, [0, 101, 0, 0]);
}

#[test]
fn opponent_finishes_on_last_tile() {
    let mut s = started();
    s.discard(0, t("Si13")).unwrap();
    s.opp_draw().unwrap();
    enter_meld(&mut s, "M6 M7 M8");
    enter_meld(&mut s, "K9 K10 K11");
    enter_meld(&mut s, "S9 S10 S11 S12 S13");
    assert!(
        s.lay_options(2).is_empty(),
        "a run of five laid this turn takes nothing more this turn"
    );
    s.players[1].hand_len = Some(1);
    s.key(t("K7")).unwrap();
    assert_eq!((s.stage, s.finisher), (Stage::Over, Some(1)));
}

#[test]
fn in_one_marked_automatically() {
    let mut s = started();
    s.discard(0, t("Si13")).unwrap();
    s.opp_draw().unwrap();
    enter_meld(&mut s, "M6 M7 M8");
    s.players[1].hand_len = Some(1);
    s.key(t("K7")).unwrap();
    assert_eq!(
        (s.stage, s.finisher, s.fin_in_one),
        (Stage::Over, Some(1), true),
        "21 points and out: elden needs no 101"
    );

    let mut s = started();
    s.hand = "K1 K2 K3 M9 M10 M11 S4".split_whitespace().map(t).collect();
    s.mode = Mode::Manual;
    for meld in ["K1 K2 K3", "M9 M10 M11"] {
        s.buffer = meld.split_whitespace().map(t).collect();
        s.lay_meld().unwrap();
    }
    assert!(!s.opening_complete() && s.may_discard(), "36 points, but the last tile may go");
    s.discard(0, t("S4")).unwrap();
    assert_eq!((s.stage, s.finisher, s.fin_in_one), (Stage::Over, Some(0), true));

    let mut s = started();
    s.discard(0, t("Si13")).unwrap();
    s.opp_draw().unwrap();
    enter_meld(&mut s, "M6 M7 M8");
    enter_meld(&mut s, "K9 K10 K11");
    enter_meld(&mut s, "S9 S10 S11 S12 S13");
    let low = *s.lay_options(0).iter().find(|o| o.tile == t("M5")).unwrap();
    s.lay_option(0, low).unwrap();
    s.players[1].hand_len = Some(1);
    s.key(t("K7")).unwrap();
    assert_eq!(
        (s.stage, s.fin_in_one),
        (Stage::Over, false),
        "opening and then laying off is a plain finish"
    );

    let mut s = started();
    s.rules.in_one_requires_open_min = true;
    s.hand = "K1 K2 K3 S4".split_whitespace().map(t).collect();
    s.mode = Mode::Manual;
    s.buffer = vec![t("K1"), t("K2"), t("K3")];
    s.lay_meld().unwrap();
    assert!(s.discard(0, t("S4")).is_err(), "this table wants 101 even for elden");
}

#[test]
fn apply_decision_discards() {
    let mut s = started();
    let v = s.view().unwrap();
    let mut a = okey_engine::analysis::Analyzer::new(v, 1).unwrap();
    a.step(20);
    let best = a.report().candidates[0].clone();
    s.apply_move(&best.mv.expect("the leading move carries its steps")).unwrap();
    assert!(s.turn == 1 || s.stage == Stage::Over);
    assert_eq!(
        s.hand.len(),
        21 - s.table.melds.iter().map(|m| m.shape.len() as usize).sum::<usize>()
    );
}

#[test]
fn totals_in_thirds() {
    assert_eq!(count(101, false), "101");
    assert_eq!(count(101, true), "33 yan 2");
    assert_eq!(count(102, true), "34");
    assert_eq!(count(117, true), "39");
    assert_eq!(count(103, true), "34 yan 1");
}
