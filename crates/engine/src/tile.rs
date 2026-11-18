pub const NCOLORS: u8 = 4;
pub const NNAT: usize = 52;
pub const JOKER: u8 = 52;
pub const NIDS: usize = 53;

pub type Tile = u8;
pub type Counts = [u8; NIDS];

pub const COLOR_NAMES: [&str; 4] = ["Kırmızı", "Sarı", "Mavi", "Siyah"];
pub const COLOR_SHORT: [&str; 4] = ["K", "S", "M", "Si"];

#[inline]
pub const fn id(color: u8, num: u8) -> Tile {
    color * 13 + num - 1
}

#[inline]
pub const fn color(t: Tile) -> u8 {
    t / 13
}

#[inline]
pub const fn num(t: Tile) -> u8 {
    t % 13 + 1
}

pub fn okey_of(indicator: Tile) -> Tile {
    let n = num(indicator);
    id(color(indicator), if n == 13 { 1 } else { n + 1 })
}

pub fn full_set() -> Counts {
    [2; NIDS]
}

pub fn total(c: &Counts) -> u32 {
    c.iter().map(|&x| x as u32).sum()
}

pub fn name(t: Tile) -> String {
    if t == JOKER {
        "Okey".to_string()
    } else {
        format!("{} {}", COLOR_NAMES[color(t) as usize], num(t))
    }
}

pub fn short(t: Tile) -> String {
    if t == JOKER {
        "OK".to_string()
    } else {
        format!("{}{}", COLOR_SHORT[color(t) as usize], num(t))
    }
}

pub fn parse(s: &str) -> Option<Tile> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("ok") || s.eq_ignore_ascii_case("okey") {
        return Some(JOKER);
    }
    let (c, rest) = if let Some(r) = s.strip_prefix("Si").or_else(|| s.strip_prefix("si")) {
        (3, r)
    } else {
        let first = s.chars().next()?;
        let c = match first.to_ascii_uppercase() {
            'K' => 0,
            'S' => 1,
            'M' => 2,
            _ => return None,
        };
        (c, &s[1..])
    };
    let n: u8 = rest.parse().ok()?;
    (1..=13).contains(&n).then(|| id(c, n))
}

pub fn counts_from(tiles: &[Tile]) -> Counts {
    let mut c = [0u8; NIDS];
    for &t in tiles {
        c[t as usize] += 1;
    }
    c
}

pub fn tiles_of(c: &Counts) -> Vec<Tile> {
    let mut v = Vec::with_capacity(total(c) as usize);
    for (i, &n) in c.iter().enumerate() {
        for _ in 0..n {
            v.push(i as Tile);
        }
    }
    v
}
