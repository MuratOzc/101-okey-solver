use okey_engine::analysis::{Decision, Phase, PlayerView, View};
use okey_engine::game::{Opened, Outcome};
use okey_engine::meld::{Shape, Solver, hand_penalty};
use okey_engine::moves::Move;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use okey_engine::stakes::Stakes;
use okey_engine::table::{JokerSlot, LayOption, Table, TableMeld, classify};
use okey_engine::tile::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Stage {
    Setup,
    Play,
    Over,
}

/// The single thing the app is asking for right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Mode {
    Gosterge,
    Starter,
    Hand,
    /// Waiting on the current player's next action.
    Idle,
    PickDraw,
    MeldEntry,
    PairEntry {
        owner: u8,
        okey: bool,
    },
    MeldMenu(usize),
    Manual,
}

pub const SEAT_NAMES: [&str; 4] = ["Ben", "Sağ", "Karşı", "Sol"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub rules: Rules,
    pub stage: Stage,
    pub gosterge: Option<Tile>,
    pub hand: Vec<Tile>,
    pub players: Vec<PlayerView>,
    pub table: Table,
    pub deck_count: u8,
    pub turn: u8,
    pub starter: u8,
    pub drawn: bool,
    pub taken: Option<Tile>,
    pub opened_this_turn: bool,
    pub first_turn: bool,
    pub buffer: Vec<Tile>,
    pub mode: Mode,
    pub result: Option<String>,
    pub model: bool,
    pub hand_no: u8,
    pub hands_total: u8,
    pub totals: [i32; 4],
    pub history: Vec<[i32; 4]>,
    pub penalties: [i32; 4],
    pub big_pairs_paid: bool,
    pub finisher: Option<u8>,
    pub fin_okey: bool,
    pub fin_elden: bool,
    pub left: [Option<i32>; 4],
    pub okeys_left: [u8; 4],
}

impl Default for State {
    fn default() -> Self {
        State {
            rules: Rules::default(),
            stage: Stage::Setup,
            gosterge: None,
            hand: vec![],
            players: vec![PlayerView::default(); 4],
            table: Table::default(),
            deck_count: 0,
            turn: 0,
            starter: 0,
            drawn: false,
            taken: None,
            opened_this_turn: false,
            first_turn: true,
            buffer: vec![],
            mode: Mode::Gosterge,
            result: None,
            model: true,
            hand_no: 1,
            hands_total: 11,
            totals: [0; 4],
            history: vec![],
            penalties: [0; 4],
            big_pairs_paid: false,
            finisher: None,
            fin_okey: false,
            fin_elden: false,
            left: [None; 4],
            okeys_left: [0; 4],
        }
    }
}

type R = Result<(), String>;

// The comeback measurements showed a gain from playing for the match only in the last hands.
const MATCH_HANDS: u8 = 2;

fn remove_one(v: &mut Vec<Tile>, t: Tile) -> bool {
    match v.iter().position(|&x| x == t) {
        Some(i) => {
            v.remove(i);
            true
        }
        None => false,
    }
}

impl State {
    pub fn new_hand(&self) -> State {
        let mut totals = self.totals;
        let mut history = self.history.clone();
        if let Some(sc) = self.hand_scores() {
            for (t, x) in totals.iter_mut().zip(sc) {
                *t += x;
            }
            history.push(sc);
        }
        State {
            rules: self.rules.clone(),
            starter: (self.starter + 1) % 4,
            model: self.model,
            hand_no: (self.hand_no + 1).min(self.hands_total),
            hands_total: self.hands_total,
            totals,
            history,
            ..State::default()
        }
    }

    pub fn new_match(&self) -> State {
        State {
            rules: self.rules.clone(),
            model: self.model,
            hands_total: self.hands_total,
            ..State::default()
        }
    }

    /// None until every opened opponent's leftover points are entered.
    pub fn hand_scores(&self) -> Option<[i32; 4]> {
        if self.stage != Stage::Over {
            return None;
        }
        let r = &self.rules;
        let mult = self.finisher.map_or(1, |f| {
            let mut m = 1;
            if self.fin_okey {
                m *= r.mult_okey_finish;
            }
            if self.fin_elden {
                m *= r.mult_elden;
            }
            if self.players[f as usize].opened == Opened::Pairs {
                m *= r.mult_pair_finish;
            }
            m
        });
        let mut out = [0; 4];
        for p in 0..4u8 {
            let finished = self.finisher.is_some_and(|f| f == p || (r.partners && (f + 2) % 4 == p));
            let pv = &self.players[p as usize];
            let base = if finished {
                r.score_finish
            } else if p == 0 {
                let c = counts_from(&self.hand);
                match pv.opened {
                    Opened::No => r.score_unopened + c[JOKER as usize] as i32 * r.joker_in_hand,
                    Opened::Melds => hand_penalty(&c, r),
                    Opened::Pairs => hand_penalty(&c, r) * r.pair_hand_mult,
                }
            } else {
                match pv.opened {
                    Opened::No => r.score_unopened + self.okeys_left[p as usize] as i32 * r.joker_in_hand,
                    Opened::Melds => self.left[p as usize]?,
                    Opened::Pairs => self.left[p as usize]? * r.pair_hand_mult,
                }
            };
            out[p as usize] = base * mult + self.penalties[p as usize];
        }
        Some(out)
    }

    /// Positive: I am ahead of that opponent (or, with partners, our team of theirs).
    pub fn lead(&self) -> [i32; 3] {
        let t = self.totals;
        if self.rules.partners {
            [(t[1] + t[3]) - (t[0] + t[2]); 3]
        } else {
            [t[1] - t[0], t[2] - t[0], t[3] - t[0]]
        }
    }

    pub fn expected_hand(&self) -> usize {
        self.rules.hand_size as usize + if self.starter == 0 { self.rules.starter_extra as usize } else { 0 }
    }

    pub fn seen(&self) -> Counts {
        let mut c = counts_from(&self.hand);
        if let Some(g) = self.gosterge {
            c[g as usize] += 1;
        }
        for p in &self.players {
            for &t in p.discards.iter().chain(&p.known) {
                c[t as usize] += 1;
            }
        }
        for t in self.table.tiles() {
            c[t as usize] += 1;
        }
        c
    }

    /// Whether the player whose turn it is may lay tiles or discard (has drawn, or starts the hand).
    pub fn may_act(&self) -> bool {
        self.drawn || self.first_turn
    }

    pub fn cur(&self) -> &PlayerView {
        &self.players[self.turn as usize]
    }

    /// How many more copies of `t` the current player could still produce.
    fn available(&self, t: Tile) -> u8 {
        let in_buffer = self.buffer.iter().filter(|&&b| b == t).count() as u8;
        let have = if self.turn == 0 {
            self.hand.iter().filter(|&&h| h == t).count() as u8
        } else {
            let known = self.cur().known.iter().filter(|&&k| k == t).count() as u8;
            2u8.saturating_sub(self.seen()[t as usize]) + known
        };
        have.saturating_sub(in_buffer)
    }

    /// Whether the keyboard key for `t` makes sense in the current mode.
    pub fn key_enabled(&self, t: Tile) -> bool {
        if !self.keyboard_visible() {
            return false;
        }
        match self.mode {
            Mode::Gosterge => t != JOKER,
            Mode::Hand | Mode::PickDraw => self.seen()[t as usize] < 2,
            Mode::Idle | Mode::MeldEntry => self.available(t) > 0,
            Mode::PairEntry { okey: true, .. } => t != JOKER && self.available(t) > 0 && self.available(JOKER) > 0,
            Mode::PairEntry { okey: false, .. } => t != JOKER && self.available(t) >= 2,
            _ => false,
        }
    }

    pub fn keyboard_visible(&self) -> bool {
        match (self.stage, self.mode) {
            (Stage::Setup, Mode::Gosterge | Mode::Hand) => true,
            (Stage::Play, Mode::PickDraw | Mode::PairEntry { .. }) => true,
            (Stage::Play, Mode::MeldEntry) => self.turn != 0,
            (Stage::Play, Mode::Idle) => self.turn != 0 && self.may_act(),
            _ => false,
        }
    }

    pub fn key(&mut self, t: Tile) -> R {
        if !self.key_enabled(t) {
            return Err(format!("{} şu an girilemez", name(t)));
        }
        match self.mode {
            Mode::Gosterge => {
                self.gosterge = Some(t);
                self.mode = if self.hand.is_empty() { Mode::Starter } else { Mode::Hand };
                Ok(())
            }
            Mode::Hand => {
                if self.hand.len() >= self.expected_hand() {
                    return Err(format!("Elinde {} taş olmalı", self.expected_hand()));
                }
                self.hand.push(t);
                self.hand.sort_unstable();
                Ok(())
            }
            Mode::PickDraw => self.my_draw(t),
            Mode::Idle => self.discard(self.turn, t),
            Mode::MeldEntry => {
                self.buffer.push(t);
                Ok(())
            }
            Mode::PairEntry { owner, okey } => self.lay_pair(owner, t, okey),
            _ => Ok(()),
        }
    }

    pub fn remove_from_hand(&mut self, i: usize) {
        if i < self.hand.len() {
            self.hand.remove(i);
        }
    }

    pub fn set_starter(&mut self, p: u8) {
        self.starter = p;
        self.hand.truncate(self.expected_hand());
        self.mode = Mode::Hand;
    }

    pub fn start(&mut self) -> R {
        let Some(g) = self.gosterge else {
            return Err("Önce göstergeyi seç".into());
        };
        if self.hand.len() != self.expected_hand() {
            return Err(format!("Elinde {} taş olmalı, şu an {}", self.expected_hand(), self.hand.len()));
        }
        let dealt = self.rules.hand_size as u32 * 4 + self.rules.starter_extra as u32;
        self.deck_count = (106 - dealt - 1) as u8;
        for p in 1..4u8 {
            let extra = if self.starter == p { self.rules.starter_extra } else { 0 };
            self.players[p as usize].hand_len = Some(self.rules.hand_size + extra);
        }
        self.players[0].gosterge_bonus = self.hand.contains(&g);
        self.turn = self.starter;
        self.first_turn = true;
        self.drawn = false;
        self.stage = Stage::Play;
        self.mode = Mode::Idle;
        Ok(())
    }

    fn may_draw(&self) -> R {
        if self.drawn {
            return Err("Bu turda zaten taş çekildi".into());
        }
        if self.first_turn {
            return Err("Başlayan ilk turda çekmez".into());
        }
        Ok(())
    }

    pub fn opp_draw(&mut self) -> R {
        self.may_draw()?;
        if self.deck_count == 0 {
            return Err("Deste bitti".into());
        }
        let p = self.turn as usize;
        self.deck_count -= 1;
        self.players[p].hand_len = Some(self.players[p].hand_len.unwrap_or(21) + 1);
        self.drawn = true;
        Ok(())
    }

    pub fn prev(p: u8) -> u8 {
        (p + 3) % 4
    }

    pub fn side_tile(&self) -> Option<Tile> {
        self.players[Self::prev(self.turn) as usize].discards.last().copied()
    }

    /// Whether I may take the side tile: I must be able to use it this turn.
    pub fn can_take(&self) -> bool {
        if self.turn != 0 || self.may_act() {
            return false;
        }
        let (Some(t), Some(view)) = (self.side_tile(), self.view()) else {
            return false;
        };
        if !self.rules.take_must_use {
            return true;
        }
        let mut g = view.determinize(&mut Rng::new(3));
        g.table = self.table.clone();
        g.can_use_taken(&mut Solver::new(&self.rules), 0, t)
    }

    pub fn take_side(&mut self) -> R {
        self.may_draw()?;
        let from = Self::prev(self.turn) as usize;
        let Some(t) = self.players[from].discards.pop() else {
            return Err("Yerde alınacak taş yok".into());
        };
        let p = self.turn as usize;
        if p == 0 {
            self.hand.push(t);
            self.hand.sort_unstable();
        } else {
            self.players[p].known.push(t);
            self.players[p].hand_len = Some(self.players[p].hand_len.unwrap_or(21) + 1);
        }
        self.taken = Some(t);
        self.drawn = true;
        self.mode = Mode::Idle;
        Ok(())
    }

    pub fn my_draw(&mut self, t: Tile) -> R {
        self.may_draw()?;
        if self.deck_count == 0 {
            return Err("Deste bitti".into());
        }
        self.hand.push(t);
        self.hand.sort_unstable();
        self.deck_count -= 1;
        self.drawn = true;
        self.mode = Mode::Idle;
        Ok(())
    }

    pub fn discard(&mut self, p: u8, t: Tile) -> R {
        if p != self.turn {
            return Err("Sıra o oyuncuda değil".into());
        }
        if !self.may_act() {
            return Err("Önce taş çekilmeli (desteden ya da yandan)".into());
        }
        let finishing = p == 0 && self.hand.len() == 1;
        let pen = if finishing {
            0
        } else if t == JOKER {
            self.rules.penalty_discard_joker
        } else if (self.cur().opened != Opened::No || self.rules.islek_applies_unopened) && self.table.is_islek(t, &self.rules) {
            self.rules.penalty_islek
        } else {
            0
        };
        if !self.opening_complete() && !finishing {
            let progress = self.opening_progress().unwrap_or_default();
            return Err(format!(
                "{progress}. Açılış tamamlanmadan taş atılamaz: eksik peri gir ya da Geri al."
            ));
        }
        if p == 0 {
            if !remove_one(&mut self.hand, t) {
                return Err(format!("Elinde {} yok", name(t)));
            }
            if self.hand.is_empty() {
                self.finish(0, Some(t));
                return Ok(());
            }
        } else {
            let pv = &mut self.players[p as usize];
            remove_one(&mut pv.known, t);
            pv.hand_len = Some(pv.hand_len.unwrap_or(22).saturating_sub(1));
            if pv.hand_len == Some(0) {
                self.finish(p, Some(t));
                return Ok(());
            }
        }
        self.penalties[p as usize] += pen;
        self.players[p as usize].discards.push(t);
        self.next_turn();
        Ok(())
    }

    fn next_turn(&mut self) {
        let pv = self.cur();
        if self.opened_this_turn
            && !self.big_pairs_paid
            && pv.opened == Opened::Pairs
            && self.rules.big_pairs_penalty != 0
            && pv.open_value >= self.rules.big_pairs_min as i32
        {
            self.penalties[self.turn as usize] -= self.rules.big_pairs_penalty;
        }
        self.big_pairs_paid = false;
        self.turn = (self.turn + 1) % 4;
        self.drawn = false;
        self.taken = None;
        self.opened_this_turn = false;
        self.first_turn = false;
        self.buffer.clear();
        self.table.new_turn();
        self.mode = Mode::Idle;
        if self.deck_count == 0 {
            self.deck_out();
        }
    }

    /// Remove `tiles` from the current player (my hand, or an opponent's known tiles and count).
    fn spend(&mut self, tiles: &[Tile]) -> R {
        let p = self.turn as usize;
        if p == 0 {
            let mut h = self.hand.clone();
            for &t in tiles {
                if !remove_one(&mut h, t) {
                    return Err(format!("Elinde {} yok", name(t)));
                }
            }
            self.hand = h;
        } else {
            let pv = &mut self.players[p];
            for &t in tiles {
                remove_one(&mut pv.known, t);
            }
            pv.hand_len = Some(pv.hand_len.unwrap_or(21).saturating_sub(tiles.len() as u8));
        }
        if self.taken.is_some_and(|t| tiles.contains(&t)) {
            self.taken = None;
        }
        Ok(())
    }

    /// What the current player's opening must reach: (meld points, pair count).
    pub fn opening_need(&self) -> (i32, u8) {
        let r = &self.rules;
        let (mut meld, mut pair) = (r.open_min, r.pair_min);
        if r.katlamali {
            for (i, p) in self.players.iter().enumerate() {
                if i == self.turn as usize && self.opened_this_turn {
                    continue;
                }
                match p.opened {
                    Opened::Melds => meld = meld.max(p.open_value + r.raise_meld_by),
                    Opened::Pairs => pair = pair.max(p.open_value as u8 + r.raise_pair_by),
                    Opened::No => {}
                }
            }
        }
        (meld, pair)
    }

    /// False while the current player has started opening this turn but is still short of the threshold.
    pub fn opening_complete(&self) -> bool {
        if !self.opened_this_turn {
            return true;
        }
        let (meld, pair) = self.opening_need();
        match self.cur().opened {
            Opened::Melds => self.cur().open_value >= meld,
            Opened::Pairs => self.cur().open_value >= pair as i32,
            Opened::No => true,
        }
    }

    /// "87 / 101" style progress for an opening in progress.
    pub fn opening_progress(&self) -> Option<String> {
        if !self.opened_this_turn {
            return None;
        }
        let (meld, pair) = self.opening_need();
        Some(match self.cur().opened {
            Opened::Pairs => format!("Açılış: {} / {} çift", self.cur().open_value, pair),
            _ => format!("Açılış: {} / {} puan", self.cur().open_value, meld),
        })
    }

    pub fn buffer_shape(&self) -> Option<Shape> {
        classify(&self.buffer, &self.rules)
    }

    /// Why the buffer cannot be laid as a new meld right now, if it cannot.
    pub fn meld_problem(&self) -> Option<String> {
        if self.buffer.is_empty() {
            return Some("Taşlara sırayla dokun".into());
        }
        let Some(shape) = self.buffer_shape() else {
            return Some("Henüz geçerli bir per değil".into());
        };
        let as_pair = shape.is_pair();
        match self.cur().opened {
            Opened::Melds if as_pair => Some("Perle açan çift indiremez (çift açana işleyebilir)".into()),
            Opened::Pairs if !as_pair && !self.rules.pair_opener_can_meld => Some("Çiftle açan seri/grup indiremez".into()),
            Opened::Pairs if as_pair && !self.opened_this_turn && self.table.pair_room(self.turn, &self.rules) == 0 => {
                Some(format!("Bu tur en fazla {} çift", self.rules.pairs_per_turn))
            }
            _ if matches!(shape, Shape::Run { .. }) && shape.len() > self.rules.max_run_len => {
                Some(format!("Yeni seri en fazla {} taş", self.rules.max_run_len))
            }
            _ => None,
        }
    }

    /// The current player lays the buffer down as a new meld (or pair).
    pub fn lay_meld(&mut self) -> R {
        if let Some(why) = self.meld_problem() {
            return Err(why);
        }
        let p = self.turn;
        let shape = self.buffer_shape().ok_or("Geçersiz per")?;
        let as_pair = shape.is_pair();
        let tiles = self.buffer.clone();
        self.spend(&tiles)?;
        let value = if as_pair { 1 } else { shape.open_value(&self.rules) };
        let opening = self.opened_this_turn;
        let pv = &mut self.players[p as usize];
        if pv.opened == Opened::No {
            pv.opened = if as_pair { Opened::Pairs } else { Opened::Melds };
            pv.open_value = value;
            self.opened_this_turn = true;
        } else if opening {
            pv.open_value += value;
        } else if as_pair {
            self.table.pairs_added[p as usize] += 1;
        }
        self.table.melds.push(TableMeld::laid(p, shape));
        self.buffer.clear();
        Ok(())
    }

    pub fn can_lay_off(&self) -> bool {
        if self.stage != Stage::Play || !self.may_act() || !self.opening_complete() {
            return false;
        }
        match self.cur().opened {
            Opened::No => false,
            Opened::Melds => true,
            Opened::Pairs => self.rules.pair_opener_can_layoff,
        }
    }

    /// Single tiles the current player could lay on meld `idx`.
    pub fn lay_options(&self, idx: usize) -> Vec<LayOption> {
        if !self.can_lay_off() {
            return vec![];
        }
        let Some(m) = self.table.melds.get(idx) else { return vec![] };
        m.options(&self.rules).into_iter().filter(|o| self.available(o.tile) > 0).collect()
    }

    pub fn lay_option(&mut self, idx: usize, o: LayOption) -> R {
        if !self.lay_options(idx).contains(&o) {
            return Err("Bu taş bu pere işlenemez".into());
        }
        if self.table.lay_off_on(idx, o.tile, Some(o.side), &self.rules).is_none() {
            return Err("Bu taş bu pere işlenemez".into());
        }
        self.spend(&[o.tile])
    }

    /// Okey swaps the current player could make on meld `idx`.
    pub fn swap_options(&self, idx: usize) -> Vec<JokerSlot> {
        let opened = self.cur().opened != Opened::No && self.opening_complete();
        if !self.rules.joker_retrieve || self.stage != Stage::Play || !self.may_act() || !opened {
            return vec![];
        }
        self.table
            .joker_slots(&self.rules)
            .into_iter()
            .filter(|s| s.meld == idx && s.tiles.iter().all(|&t| self.available(t) > 0))
            .collect()
    }

    pub fn swap(&mut self, slot: &JokerSlot) -> R {
        if !self.swap_options(slot.meld).contains(slot) {
            return Err("Bu perden okey alınamaz".into());
        }
        let tiles = slot.tiles.clone();
        self.spend(&tiles)?;
        let owner = self.table.melds[slot.meld].owner;
        if owner != self.turn && !(self.rules.partners && owner == (self.turn + 2) % 4) {
            self.penalties[owner as usize] += self.rules.okey_lost_penalty;
        }
        self.table.swap_joker(slot);
        for _ in 0..slot.jokers {
            if self.turn == 0 {
                self.hand.push(JOKER);
            } else {
                let pv = &mut self.players[self.turn as usize];
                pv.known.push(JOKER);
                pv.hand_len = Some(pv.hand_len.unwrap_or(21) + 1);
            }
        }
        self.hand.sort_unstable();
        Ok(())
    }

    /// Whether the current player may add pairs to `owner`'s pair area this turn.
    pub fn can_add_pair(&self, owner: u8) -> bool {
        if self.stage != Stage::Play || !self.may_act() || !self.opening_complete() || self.table.pair_room(owner, &self.rules) == 0 {
            return false;
        }
        match self.cur().opened {
            Opened::No => false,
            Opened::Pairs => owner == self.turn,
            Opened::Melds => self.rules.pairs_onto_pair_opener,
        }
    }

    fn lay_pair(&mut self, owner: u8, t: Tile, okey: bool) -> R {
        if !self.can_add_pair(owner) {
            return Err(format!("Bu tur en fazla {} çift işlenir", self.rules.pairs_per_turn));
        }
        let tiles = if okey { vec![t, JOKER] } else { vec![t, t] };
        self.spend(&tiles)?;
        self.table.pairs_added[owner as usize] += 1;
        self.table.melds.push(TableMeld::new(owner, Shape::Pair { tile: t, joker: okey }));
        Ok(())
    }

    /// Whether tapping meld `idx` offers the current player anything.
    pub fn meld_tappable(&self, idx: usize) -> bool {
        let Some(m) = self.table.melds.get(idx) else { return false };
        !self.lay_options(idx).is_empty() || !self.swap_options(idx).is_empty() || (m.shape.is_pair() && self.can_add_pair(m.owner))
    }

    pub fn finish(&mut self, p: u8, last: Option<Tile>) {
        if let Some(t) = last {
            if p == 0 {
                remove_one(&mut self.hand, t);
            }
            self.players[p as usize].discards.push(t);
        }
        self.result = Some(format!("{} bitirdi", SEAT_NAMES[p as usize]));
        self.finisher = Some(p);
        self.fin_okey = last == Some(JOKER);
        self.stage = Stage::Over;
        self.mode = Mode::Idle;
    }

    pub fn deck_out(&mut self) {
        self.result = Some("Deste bitti, kimse bitiremedi".into());
        self.stage = Stage::Over;
        self.mode = Mode::Idle;
    }

    pub fn match_decides(&self) -> bool {
        self.hands_total.saturating_sub(self.hand_no) < MATCH_HANDS && !(self.history.is_empty() && self.totals == [0; 4])
    }

    pub fn stakes(&self) -> Option<Stakes> {
        self.match_decides().then(|| Stakes {
            hands_left: self.hands_total.saturating_sub(self.hand_no),
            behind: self.lead().map(|x| -x),
        })
    }

    /// The analysis input, when it is my turn.
    pub fn view(&self) -> Option<View> {
        if self.stage != Stage::Play || self.turn != 0 || !self.opening_complete() {
            return None;
        }
        self.view_as(if self.may_act() { Phase::AfterDraw } else { Phase::BeforeDraw })
    }

    pub fn view_as(&self, phase: Phase) -> Option<View> {
        Some(View {
            rules: self.rules.clone(),
            gosterge: self.gosterge?,
            hand: self.hand.clone(),
            players: self.players.clone(),
            table: self.table.melds.clone(),
            pairs_added: self.table.pairs_added,
            deck_count: self.deck_count,
            phase,
            taken: self.taken,
            stakes: self.stakes(),
        })
    }

    /// Carry out an explicit move (steps, then discard) for me on the real state.
    pub fn apply_move(&mut self, mv: &Move) -> R {
        let view = self.view().ok_or("Sıra sende değil")?;
        let mut g = view.determinize(&mut Rng::new(3));
        let before = g.table.melds.len();
        let was_unopened = self.players[0].opened == Opened::No;
        let res = g
            .play_move(0, mv, self.taken)
            .map_err(|_| "Bu hamle artık geçerli değil".to_string())?;
        let laid: i32 = g.table.melds[before..]
            .iter()
            .filter(|m| m.owner == 0)
            .map(|m| if m.shape.is_pair() { 1 } else { m.shape.open_value(&self.rules) })
            .sum();
        self.table = g.table.clone();
        self.hand = tiles_of(&g.players[0].hand);
        self.players[0].opened = g.players[0].opened;
        if was_unopened && g.players[0].opened != Opened::No {
            self.players[0].open_value = laid;
        }
        for (mine, sim) in self.penalties.iter_mut().zip(&g.players) {
            *mine += sim.penalty;
        }
        self.big_pairs_paid = true;
        self.taken = None;
        self.buffer.clear();
        self.mode = Mode::Idle;
        match res {
            Some(Outcome::Finished(f)) => {
                self.hand.push(f.last);
                self.finish(0, Some(f.last));
                self.fin_okey = f.okey;
                self.fin_elden = f.elden;
            }
            Some(Outcome::DeckOut) => self.deck_out(),
            None => {
                self.players[0].discards.push(mv.discard);
                self.next_turn();
            }
        }
        Ok(())
    }

    /// Carry out an analysed decision for me on the real state.
    pub fn apply(&mut self, d: Decision) -> R {
        let view = self.view().ok_or("Sıra sende değil")?;
        match d {
            Decision::Take => self.take_side(),
            Decision::Draw => {
                self.may_draw()?;
                self.mode = Mode::PickDraw;
                Ok(())
            }
            Decision::Play(_) => Err("Bu hamlenin adımları eksik".into()),
            Decision::Act { choice, discard } => {
                let mut sv = Solver::new(&self.rules);
                let mut g = view.determinize(&mut Rng::new(3));
                let before = g.table.melds.len();
                let was_unopened = self.players[0].opened == Opened::No;
                let res = g
                    .apply_choice(&mut sv, 0, choice, self.taken)
                    .map_err(|_| "Bu hamle artık geçerli değil".to_string())?;
                let laid: i32 = g.table.melds[before..]
                    .iter()
                    .filter(|m| m.owner == 0)
                    .map(|m| if m.shape.is_pair() { 1 } else { m.shape.open_value(&self.rules) })
                    .sum();
                self.table = g.table.clone();
                self.hand = tiles_of(&g.players[0].hand);
                self.players[0].opened = g.players[0].opened;
                if was_unopened && g.players[0].opened != Opened::No {
                    self.players[0].open_value = laid;
                    self.opened_this_turn = true;
                }
                for (mine, sim) in self.penalties.iter_mut().zip(&g.players) {
                    *mine += sim.penalty;
                }
                self.big_pairs_paid = true;
                self.taken = None;
                self.buffer.clear();
                self.mode = Mode::Idle;
                match res {
                    Some(Outcome::Finished(f)) => {
                        self.hand.push(f.last);
                        self.finish(0, Some(f.last));
                        self.fin_okey = f.okey;
                        self.fin_elden = f.elden;
                        Ok(())
                    }
                    Some(Outcome::DeckOut) => {
                        self.deck_out();
                        Ok(())
                    }
                    None => {
                        let t = discard.ok_or("Atılacak taş seçilmedi")?;
                        self.discard(0, t)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
        s.take_side().unwrap();
        assert_eq!(s.players[3].known, vec![t("M7")]);
        s.key(t("Si8")).unwrap();
        assert_eq!(s.players[3].known, vec![t("M7")], "a taken tile stays known until it is shown");
        assert!(s.view().is_some_and(|v| v.phase == Phase::BeforeDraw));
        assert!(!s.can_take(), "Si8 fits no meld of mine, so it cannot be used to open");
        s.apply(Decision::Draw).unwrap();
        assert_eq!(s.mode, Mode::PickDraw);
        s.key(t("K6")).unwrap();
        assert!(s.view().is_some_and(|v| v.phase == Phase::AfterDraw));
    }

    #[test]
    fn usable_side_tile_can_be_taken() {
        let mut s = started();
        s.discard(0, t("Si13")).unwrap();
        for d in ["K7", "M7", "K9"] {
            s.opp_draw().unwrap();
            s.key(t(d)).unwrap();
        }
        assert!(s.can_take(), "K9 makes a group of nines and still opens with 105");
    }

    #[test]
    fn opponent_lays_then_layoff_options() {
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
    fn nothing_touches_the_table_until_the_opening_reaches_101() {
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
    fn my_own_half_opening_stops_the_analysis() {
        let mut s = started();
        s.mode = Mode::Manual;
        s.buffer = vec![t("K1"), t("K2"), t("K3")];
        s.lay_meld().unwrap();
        assert!(!s.opening_complete());
        assert!(s.view().is_none(), "no advice may be computed from an illegal half-opened position");
        assert!(s.discard(0, t("S9")).is_err());
    }

    #[test]
    fn katlamali_opening_must_beat_the_previous_one() {
        let mut s = started();
        s.rules.katlamali = true;
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
    fn match_situation_reaches_the_analysis_and_carries_to_the_next_hand() {
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
    fn disabled_keys_follow_what_is_possible() {
        let mut s = started();
        assert!(!s.key_enabled(t("K1")), "no keyboard input on my own action turn");
        s.discard(0, t("Si13")).unwrap();
        s.opp_draw().unwrap();
        assert!(s.key_enabled(t("K7")));
        assert!(!s.key_enabled(t("S1")), "both S1 are in my hand");
    }

    #[test]
    fn the_hand_ends_once_the_last_tile_is_drawn_and_discarded() {
        let mut s = started();
        s.discard(0, t("Si13")).unwrap();
        s.deck_count = 1;
        s.opp_draw().unwrap();
        s.key(t("K7")).unwrap();
        assert_eq!(s.stage, Stage::Over, "nobody may take the side tile once the deck is empty");
    }

    #[test]
    fn the_hand_is_scored_and_added_to_the_match() {
        let mut s = started();
        s.discard(0, t("Si13")).unwrap();
        s.opp_draw().unwrap();
        enter_meld(&mut s, "M6 M7 M8");
        enter_meld(&mut s, "K9 K10 K11");
        enter_meld(&mut s, "S9 S10 S11 S12 S13");
        s.finish(1, Some(JOKER));
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
    fn penalties_are_written_as_they_happen() {
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
    fn an_opponent_finishes_by_discarding_the_last_tile() {
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
}
