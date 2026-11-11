use crate::meld::Shape;
use crate::rules::Rules;
use crate::tile::*;

#[derive(Clone, Debug, Default)]
pub struct PairPlan {
    pub pairs: Vec<Shape>,
    pub rest: Vec<Tile>,
}

/// Best pairing of a hand. `gosterge` is the gösterge tile when this player may use it as a wild pair partner.
pub fn pair_plan(hand: &Counts, rules: &Rules, gosterge: Option<Tile>) -> PairPlan {
    let mut pairs = Vec::new();
    let mut singles: Vec<Tile> = Vec::new();
    for t in 0..NNAT as Tile {
        match hand[t as usize] {
            2 => pairs.push(Shape::Pair { tile: t, joker: false }),
            1 => singles.push(t),
            _ => {}
        }
    }
    singles.sort_by_key(|&t| std::cmp::Reverse(num(t)));
    let mut jokers = hand[JOKER as usize];

    if let Some(g) = gosterge.filter(|_| rules.gosterge_pair)
        && let Some(pos) = singles.iter().position(|&t| t == g)
    {
        singles.remove(pos);
        if !singles.is_empty() {
            let other = singles.remove(0);
            pairs.push(Shape::GostergePair { gosterge: g, other });
        } else {
            singles.push(g);
        }
    }
    if rules.joker_pairs_with_single {
        while jokers > 0 && !singles.is_empty() {
            let t = singles.remove(0);
            pairs.push(Shape::Pair { tile: t, joker: true });
            jokers -= 1;
        }
    }
    if rules.two_jokers_pair && jokers >= 2 {
        pairs.push(Shape::Pair { tile: JOKER, joker: true });
        jokers -= 2;
    }
    let mut rest = singles;
    rest.extend(std::iter::repeat_n(JOKER, jokers as usize));
    PairPlan { pairs, rest }
}

/// How many pairs `pair_plan` would make and how many tiles it would leave, without building them.
pub fn pair_stats(hand: &Counts, rules: &Rules, gosterge: Option<Tile>) -> (usize, usize) {
    let mut pairs = hand[..NNAT].iter().filter(|&&n| n == 2).count();
    let mut singles = hand[..NNAT].iter().filter(|&&n| n == 1).count();
    let mut jokers = hand[JOKER as usize] as usize;
    if let Some(g) = gosterge.filter(|_| rules.gosterge_pair)
        && hand[g as usize] == 1
        && singles >= 2
    {
        pairs += 1;
        singles -= 2;
    }
    if rules.joker_pairs_with_single {
        let k = jokers.min(singles);
        pairs += k;
        singles -= k;
        jokers -= k;
    }
    if rules.two_jokers_pair && jokers >= 2 {
        pairs += 1;
        jokers -= 2;
    }
    (pairs, singles + jokers)
}
