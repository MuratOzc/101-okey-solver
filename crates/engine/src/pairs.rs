use crate::meld::Shape;
use crate::rules::Rules;
use crate::tile::*;

#[derive(Clone, Debug, Default)]
pub struct PairPlan {
    pub pairs: Vec<Shape>,
    pub rest: Vec<Tile>,
}

/// Best pairing of a hand. `indicator` is the indicator tile when this player may use it as a wild pair partner.
pub fn pair_plan(hand: &Counts, rules: &Rules, indicator: Option<Tile>) -> PairPlan {
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

    if let Some(g) = indicator.filter(|_| rules.indicator_pair)
        && let Some(pos) = singles.iter().position(|&t| t == g)
    {
        singles.remove(pos);
        if !singles.is_empty() {
            let other = singles.remove(0);
            pairs.push(Shape::IndicatorPair { indicator: g, other });
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

/// The same number of pairs with each okey beside a tile whose twin is already out, as far as the hand has
/// such singles: nobody can then lay the twin and take the okey. None when that changes nothing.
pub fn guarded(plan: &PairPlan, seen: &Counts) -> Option<Vec<Shape>> {
    let mut pool: Vec<Tile> = plan.rest.iter().copied().filter(|&t| t != JOKER).collect();
    let (mut out, mut okeys, mut held_indicator) = (Vec::new(), 0, None);
    for &s in &plan.pairs {
        match s {
            Shape::Pair { tile, joker: true } if tile != JOKER => {
                pool.push(tile);
                okeys += 1;
            }
            Shape::IndicatorPair { indicator, other } => {
                pool.push(other);
                held_indicator = Some(indicator);
            }
            s => out.push(s),
        }
    }
    if okeys == 0 || !pool.iter().any(|&t| seen[t as usize] > 0) {
        return None;
    }
    pool.sort_by_key(|&t| std::cmp::Reverse(num(t)));
    for _ in 0..okeys {
        let k = pool.iter().position(|&t| seen[t as usize] > 0).unwrap_or(0);
        out.push(Shape::Pair {
            tile: pool.remove(k),
            joker: true,
        });
    }
    if let Some(indicator) = held_indicator {
        out.push(Shape::IndicatorPair {
            indicator,
            other: pool.remove(0),
        });
    }
    let sorted = |v: &[Shape]| {
        let mut v = v.to_vec();
        v.sort_unstable();
        v
    };
    (sorted(&out) != sorted(&plan.pairs)).then_some(out)
}

/// How many pairs `pair_plan` would make and how many tiles it would leave, without building them.
pub fn pair_stats(hand: &Counts, rules: &Rules, indicator: Option<Tile>) -> (usize, usize) {
    let mut pairs = hand[..NNAT].iter().filter(|&&n| n == 2).count();
    let mut singles = hand[..NNAT].iter().filter(|&&n| n == 1).count();
    let mut jokers = hand[JOKER as usize] as usize;
    if let Some(g) = indicator.filter(|_| rules.indicator_pair)
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
