//! What seat 0 knows of the hand, and the game it deals out of that.

use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerView {
    pub discards: Vec<Tile>,
    pub known: Vec<Tile>,
    pub opened: Opened,
    pub open_value: i32,
    pub hand_len: Option<u8>,
    #[serde(alias = "gosterge_bonus")]
    pub has_indicator: bool,
}

impl Default for PlayerView {
    fn default() -> Self {
        PlayerView {
            discards: vec![],
            known: vec![],
            opened: Opened::No,
            open_value: 0,
            hand_len: None,
            has_indicator: false,
        }
    }
}

/// How many copies of each tile the holder of `hand` has seen: the hand, the indicator, every
/// discard and shown tile, and the table.
pub fn seen_tiles(hand: &[Tile], indicator: Option<Tile>, players: &[PlayerView], table: &[TableMeld]) -> Counts {
    let mut c = [0u8; NIDS];
    let shown = players.iter().flat_map(|p| p.discards.iter().chain(&p.known));
    for &t in hand.iter().chain(&indicator).chain(shown) {
        c[t as usize] = c[t as usize].saturating_add(1);
    }
    for t in table.iter().flat_map(|m| m.shape.tiles()) {
        c[t as usize] = c[t as usize].saturating_add(1);
    }
    c
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    BeforeDraw,
    AfterDraw,
}

/// Everything the user has entered, from seat 0's point of view. Seat order is the play order:
/// 0 = me, 1 = right, 2 = across, 3 = left (whose discard I may take).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct View {
    pub rules: Rules,
    #[serde(alias = "gosterge")]
    pub indicator: Tile,
    pub hand: Vec<Tile>,
    pub players: Vec<PlayerView>,
    pub table: Vec<TableMeld>,
    #[serde(default)]
    pub pairs_added: [u8; 4],
    pub deck_count: u8,
    pub phase: Phase,
    pub taken: Option<Tile>,
    /// The match situation, when the player has entered it.
    #[serde(default)]
    pub stakes: Option<Stakes>,
}

impl View {
    pub fn left_top(&self) -> Option<Tile> {
        self.players.get(3).and_then(|p| p.discards.last().copied())
    }

    pub fn requirements(&self) -> (i32, u8) {
        self.rules.thresholds(self.players.iter().map(|p| (p.opened, p.open_value)))
    }

    pub fn seen(&self) -> Counts {
        seen_tiles(&self.hand, Some(self.indicator), &self.players, &self.table)
    }

    pub fn unseen(&self) -> Counts {
        let s = self.seen();
        let mut u = [0u8; NIDS];
        for i in 0..NIDS {
            u[i] = 2u8.saturating_sub(s[i]);
        }
        u
    }

    pub fn problems(&self) -> Vec<String> {
        let mut v = Vec::new();
        let s = self.seen();
        for i in 0..NIDS {
            if s[i] > 2 {
                v.push(format!("{} {} kez girilmiş (en fazla 2)", name(i as Tile), s[i]));
            }
        }
        if self.players.len() != 4 {
            v.push("4 oyuncu olmalı".into());
        }
        v
    }

    pub fn hand_len_of(&self, p: usize) -> u8 {
        if let Some(n) = self.players[p].hand_len {
            return n;
        }
        let laid: usize = self
            .table
            .iter()
            .filter(|m| m.owner as usize == p)
            .map(|m| m.shape.len() as usize)
            .sum();
        self.rules.hand_size.saturating_sub(laid as u8)
    }

    pub fn determinize(&self, rng: &mut Rng) -> Game {
        let unseen = self.unseen();
        let mut pool = tiles_of(&unseen);
        rng.shuffle(&mut pool);
        let (meld_req, pair_req) = self.requirements();
        let mut seen = [0u8; NIDS];
        seen[self.indicator as usize] += 1;
        for p in &self.players {
            p.discards.iter().for_each(|&t| seen[t as usize] += 1);
        }
        for m in &self.table {
            m.shape.tiles().into_iter().for_each(|t| seen[t as usize] += 1);
        }
        let mut players: Vec<Player> = Vec::with_capacity(4);
        for p in 0..4 {
            let pv = &self.players[p];
            let mut hand = [0u8; NIDS];
            if p == 0 {
                hand = counts_from(&self.hand);
            } else {
                for &t in &pv.known {
                    hand[t as usize] += 1;
                }
                let need = (self.hand_len_of(p) as usize).saturating_sub(pv.known.len());
                for _ in 0..need {
                    if let Some(t) = pool.pop() {
                        hand[t as usize] += 1;
                    }
                }
            }
            players.push(Player {
                hand,
                opened: pv.opened,
                has_indicator: pv.has_indicator,
                penalty: 0,
            });
        }
        pool.truncate(self.deck_count as usize);
        let top = match self.phase {
            Phase::BeforeDraw => self.left_top(),
            Phase::AfterDraw => None,
        };
        let mut piles = [[0u8; NIDS]; 4];
        let mut last = [None; 4];
        for (p, pv) in self.players.iter().enumerate().take(4) {
            pv.discards.iter().for_each(|&t| piles[p][t as usize] += 1);
            last[p] = pv.discards.last().copied();
        }
        Game {
            rules: self.rules.clone(),
            indicator: self.indicator,
            players: players.try_into().expect("4 players"),
            deck: pool,
            table: Table {
                melds: self.table.clone(),
                pairs_added: self.pairs_added,
            },
            top,
            current: 0,
            meld_req,
            pair_req,
            seen,
            piles,
            last,
            last_draw: None,
            log: None,
            policy: [Policy::default(); 4],
            stakes: self.stakes,
        }
    }
}

/// Seat 0's view of a live game: public information plus its own hand.
pub fn public_view(g: &Game, phase: Phase, taken: Option<Tile>) -> View {
    let mut players = vec![PlayerView::default(); 4];
    for p in 0..4 {
        players[p].opened = g.players[p].opened;
        // A game keeps only the thresholds: every opener gets the value that gives them back.
        players[p].open_value = match g.players[p].opened {
            Opened::Melds => g.meld_req - g.rules.raise_meld_by,
            Opened::Pairs => g.pair_req as i32 - g.rules.raise_pair_by as i32,
            Opened::No => 0,
        };
        players[p].hand_len = Some(g.hand_len(p as u8) as u8);
        let mut pile = g.piles[p];
        let last = g.last[p].filter(|&t| pile[t as usize] > 0);
        if let Some(t) = last {
            pile[t as usize] -= 1;
        }
        players[p].discards = tiles_of(&pile);
        players[p].discards.extend(last);
    }
    players[0].has_indicator = g.players[0].has_indicator;
    View {
        rules: g.rules.clone(),
        indicator: g.indicator,
        hand: tiles_of(&g.players[0].hand),
        players,
        table: g.table.melds.clone(),
        pairs_added: g.table.pairs_added,
        deck_count: g.deck.len() as u8,
        phase,
        taken,
        stakes: g.stakes,
    }
}
