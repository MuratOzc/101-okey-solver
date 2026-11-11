use serde::{Deserialize, Serialize};

/// Where the match stands before this hand, from seat 0's side. With it the analyzer plays for the
/// chance of ending the match on top instead of for this hand's points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stakes {
    /// Hands still to be played after this one.
    pub hands_left: u8,
    /// How many points seat 0 trails each opponent (right, across, left); negative when ahead.
    /// With partners only the first counts: how far my team trails the other.
    pub behind: [i32; 3],
}

/// Standard deviation of the score difference one hand makes between two players (20 000 bot hands).
const PAIR_SPREAD: f64 = 109.0;
/// The same between the two teams of a partnered game.
const TEAM_SPREAD: f64 = 248.0;

impl Stakes {
    /// The chance of ending ahead of a rival I trail by `trail` points once this hand is scored.
    /// In the last hand that is decided on the spot; earlier, the hands to come can still swing it.
    fn ahead(&self, trail: f64, spread: f64) -> f64 {
        if self.hands_left == 0 {
            return if trail < 0.0 {
                1.0
            } else if trail == 0.0 {
                0.5
            } else {
                0.0
            };
        }
        let z = -trail / (spread * (self.hands_left as f64).sqrt());
        0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2))
    }

    /// The chance that seat 0 (or its team) does not win the match, given this hand's scores by seat.
    pub fn loss(&self, s: &[i32; 4], partners: bool) -> f64 {
        if partners {
            return 1.0 - self.ahead((s[0] + s[2] - s[1] - s[3] + self.behind[0]) as f64, TEAM_SPREAD);
        }
        1.0 - (1..4)
            .map(|q| self.ahead((s[0] - s[q] + self.behind[q - 1]) as f64, PAIR_SPREAD))
            .product::<f64>()
    }
}

/// Abramowitz–Stegun 7.1.26, accurate to about 1e-7.
pub(crate) fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let poly = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let y = 1.0 - poly * (-x * x).exp();
    if x >= 0.0 { y } else { -y }
}
