use okey_engine::analysis::{Decision, Phase, PlayerView, View, seen_tiles};
use okey_engine::game::{Game, Opened, Outcome};
use okey_engine::meld::{Shape, Solver, hand_penalty};
use okey_engine::moves::Move;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use okey_engine::stakes::Stakes;
use okey_engine::table::{JokerSlot, LayOption, Side, Table, TableMeld, classify};
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
    pub touched_table: bool,
    pub first_turn: bool,
    pub buffer: Vec<Tile>,
    pub mode: Mode,
    pub result: Option<String>,
    pub model: bool,
    /// Show meld totals the way the table counts them: thirds, with what is left over as "yan".
    #[serde(default)]
    pub yan: bool,
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
            touched_table: false,
            first_turn: true,
            buffer: vec![],
            mode: Mode::Gosterge,
            result: None,
            model: true,
            yan: false,
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

/// A meld total as the table counts it: 101, or "33 yan 2".
pub fn count(v: i32, yan: bool) -> String {
    match (yan, v.div_euclid(3), v.rem_euclid(3)) {
        (false, ..) => v.to_string(),
        (true, base, 0) => base.to_string(),
        (true, base, left) => format!("{base} yan {left}"),
    }
}

// examples/season.rs: playing for the match before the last two hands won no more matches and cost points.
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
            yan: self.yan,
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
            yan: self.yan,
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
            r.finish_mult(self.fin_okey, self.fin_elden, self.players[f as usize].opened == Opened::Pairs)
        });
        let mut out = [0; 4];
        for p in 0..4u8 {
            let opened = self.players[p as usize].opened;
            let base = if self.finisher.is_some_and(|f| r.same_team(f, p)) {
                r.score_finish
            } else if p == 0 {
                let c = counts_from(&self.hand);
                r.hand_charge(opened, hand_penalty(&c, r), c[JOKER as usize] as i32)
            } else {
                let left = if opened == Opened::No { 0 } else { self.left[p as usize]? };
                r.hand_charge(opened, left, self.okeys_left[p as usize] as i32)
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
        seen_tiles(&self.hand, self.gosterge, &self.players, &self.table.melds)
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
            Mode::Gosterge => t != JOKER && self.hand.iter().filter(|&&x| x == t).count() < 2,
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
        self.penalties[p] += self.rules.take_cost(t);
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
        if let Some(x) = self.taken.filter(|_| self.rules.take_must_use) {
            return Err(format!(
                "Yandan aldığı {} bu tur kullanılmalı: onunla per aç ya da işle, değilse Geri al.",
                name(x)
            ));
        }
        let finishing = self.last_tile();
        let pen = if finishing {
            0
        } else {
            self.table.discard_penalty(t, self.cur().opened != Opened::No, &self.rules)
        };
        if !self.may_discard() {
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

    /// The reward for opening with many pairs, once the opening is complete.
    fn pay_big_pairs(&mut self) {
        let pv = self.cur();
        if self.opened_this_turn && !self.big_pairs_paid && pv.opened == Opened::Pairs {
            let reward = self.rules.big_pairs_reward(pv.open_value as u8);
            self.penalties[self.turn as usize] -= reward;
            self.big_pairs_paid = reward != 0;
        }
    }

    fn next_turn(&mut self) {
        self.pay_big_pairs();
        self.big_pairs_paid = false;
        self.turn = (self.turn + 1) % 4;
        self.drawn = false;
        self.taken = None;
        self.opened_this_turn = false;
        self.touched_table = false;
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
        let mine_in_progress = |i: usize| i == self.turn as usize && self.opened_this_turn;
        let down = self.players.iter().enumerate().filter(|&(i, _)| !mine_in_progress(i));
        self.rules.thresholds(down.map(|(_, p)| (p.opened, p.open_value)))
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

    fn last_tile(&self) -> bool {
        if self.turn == 0 {
            self.hand.len() == 1
        } else {
            self.cur().hand_len == Some(1)
        }
    }

    /// A half-laid opening may end the turn only by going out with it (elden).
    pub fn may_discard(&self) -> bool {
        self.opening_complete() || (self.last_tile() && !self.rules.elden_requires_open_min)
    }

    pub fn total(&self, v: i32) -> String {
        count(v, self.yan)
    }

    pub fn points(&self, v: i32) -> String {
        if self.yan { self.total(v) } else { format!("{v} puan") }
    }

    /// "87 / 101" style progress for an opening in progress.
    pub fn opening_progress(&self) -> Option<String> {
        if !self.opened_this_turn {
            return None;
        }
        let (meld, pair) = self.opening_need();
        Some(match self.cur().opened {
            Opened::Pairs => format!("Açılış: {} / {} çift", self.cur().open_value, pair),
            _ => format!("Açılış: {} / {}", self.total(self.cur().open_value), self.points(meld)),
        })
    }

    pub fn buffer_shape(&self) -> Option<Shape> {
        classify(&self.buffer, &self.rules).or_else(|| self.gosterge_pair())
    }

    /// The gösterge's twin with any other tile, for whoever was dealt it. Opponents are taken at
    /// their word; I only had it if it was in my dealt hand.
    fn gosterge_pair(&self) -> Option<Shape> {
        let g = self.gosterge.filter(|_| self.rules.gosterge_pair)?;
        let &[a, b] = self.buffer.as_slice() else { return None };
        let other = if a == g {
            b
        } else if b == g {
            a
        } else {
            return None;
        };
        let dealt = self.turn != 0 || self.players[0].gosterge_bonus;
        (other != JOKER && dealt).then_some(Shape::GostergePair { gosterge: g, other })
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
            _ if as_pair && !self.rules.pairs_enabled => Some("Bu kurallarda çift açılmıyor".into()),
            Opened::Melds if !as_pair && !self.opened_this_turn && !self.rules.new_melds_after_open => {
                Some("Bu kurallarda açtıktan sonra yeni per indirilmez".into())
            }
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
        if matches!(shape, Shape::GostergePair { .. }) {
            pv.gosterge_bonus = true;
        }
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
        let Some(at) = self.table.lay_off_on(idx, o.tile, Some(o.side), &self.rules) else {
            return Err("Bu taş bu pere işlenemez".into());
        };
        if o.tile == JOKER {
            self.table.melds[idx].okey_from(self.turn, at);
        }
        self.touched_table = true;
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

    /// Who would be charged if the current player took the okeys of this slot.
    pub fn okey_losers(&self, slot: &JokerSlot) -> Vec<u8> {
        let mut table = self.table.clone();
        table
            .swap_joker(slot, self.turn)
            .into_iter()
            .flatten()
            .filter(|&p| !self.rules.same_team(p, self.turn))
            .collect()
    }

    pub fn swap(&mut self, slot: &JokerSlot) -> R {
        if !self.swap_options(slot.meld).contains(slot) {
            return Err("Bu perden okey alınamaz".into());
        }
        let tiles = slot.tiles.clone();
        self.spend(&tiles)?;
        for loser in self.okey_losers(slot) {
            self.penalties[loser as usize] += self.rules.okey_lost_penalty;
        }
        self.table.swap_joker(slot, self.turn);
        self.touched_table = true;
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
        self.touched_table |= owner != self.turn;
        let mut meld = TableMeld::new(owner, Shape::Pair { tile: t, joker: okey });
        if okey {
            meld.okey_from(self.turn, Side::Group);
        }
        self.table.melds.push(meld);
        Ok(())
    }

    /// Whether tapping meld `idx` offers the current player anything.
    pub fn meld_tappable(&self, idx: usize) -> bool {
        let Some(m) = self.table.melds.get(idx) else { return false };
        !self.lay_options(idx).is_empty() || !self.swap_options(idx).is_empty() || (m.shape.is_pair() && self.can_add_pair(m.owner))
    }

    pub fn finish(&mut self, p: u8, last: Option<Tile>) {
        if p == self.turn {
            self.pay_big_pairs();
        }
        if let Some(t) = last {
            if p == 0 {
                remove_one(&mut self.hand, t);
            }
            self.players[p as usize].discards.push(t);
        }
        self.result = Some(format!("{} bitirdi", SEAT_NAMES[p as usize]));
        self.finisher = Some(p);
        self.fin_okey = last == Some(JOKER);
        self.fin_elden = p == self.turn && self.opened_this_turn && (!self.touched_table || self.rules.open_finish_is_elden);
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

    /// Take my side of a simulated turn over into the real state. `before` is how many melds the
    /// table had when the turn began.
    fn take_over(&mut self, g: &Game, before: usize) {
        let laid: i32 = g.table.melds[before..]
            .iter()
            .filter(|m| m.owner == 0)
            .map(|m| if m.shape.is_pair() { 1 } else { m.shape.open_value(&self.rules) })
            .sum();
        let opens = self.players[0].opened == Opened::No && g.players[0].opened != Opened::No;
        self.touched_table |= g.table.melds[before..].iter().any(|m| m.owner != 0)
            || g.table.melds.iter().zip(&self.table.melds).any(|(now, was)| now.shape != was.shape);
        self.table = g.table.clone();
        self.hand = tiles_of(&g.players[0].hand);
        self.players[0].opened = g.players[0].opened;
        if opens {
            self.players[0].open_value = laid;
            self.opened_this_turn = true;
            // The simulated opening has already written the reward for many pairs.
            self.big_pairs_paid = true;
        }
        for (mine, sim) in self.penalties.iter_mut().zip(&g.players) {
            *mine += sim.penalty;
        }
        self.taken = None;
        self.buffer.clear();
        self.mode = Mode::Idle;
    }

    fn hand_ended(&mut self, res: Option<Outcome>) -> bool {
        match res {
            Some(Outcome::Finished(f)) => {
                self.hand.push(f.last);
                self.finish(0, Some(f.last));
            }
            Some(Outcome::DeckOut) => self.deck_out(),
            None => return false,
        }
        true
    }

    /// Carry out an explicit move (steps, then discard) for me on the real state.
    pub fn apply_move(&mut self, mv: &Move) -> R {
        let view = self.view().ok_or("Sıra sende değil")?;
        let mut g = view.determinize(&mut Rng::new(3));
        let before = g.table.melds.len();
        let res = g
            .play_move(0, mv, self.taken)
            .map_err(|_| "Bu hamle artık geçerli değil".to_string())?;
        self.take_over(&g, before);
        if !self.hand_ended(res) {
            self.players[0].discards.push(mv.discard);
            self.next_turn();
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
                let res = g
                    .apply_choice(&mut sv, 0, choice, self.taken)
                    .map_err(|_| "Bu hamle artık geçerli değil".to_string())?;
                self.take_over(&g, before);
                if self.hand_ended(res) {
                    return Ok(());
                }
                let t = discard.ok_or("Atılacak taş seçilmedi")?;
                self.discard(0, t)
            }
        }
    }
}

#[cfg(test)]
mod tests;
