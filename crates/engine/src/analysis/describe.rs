//! Words and steps for showing a decision to the player.

use super::*;

pub fn label(d: &Decision) -> String {
    match d {
        Decision::Draw => "Desteden çek".into(),
        Decision::Take => "Yandan al".into(),
        Decision::Play(_) => "Hamle".into(),
        Decision::Act { choice, discard } => {
            let lay_note = |l: &LayStyle| match l {
                LayStyle::Nothing => ", başka bir şey indirme",
                LayStyle::AllSafe => ", okeyi seriye koyma",
                LayStyle::All => "",
            };
            let what = match choice {
                OpenChoice::Stay => "Açma".to_string(),
                OpenChoice::OpenMelds(style, lay) => {
                    let how = match style {
                        OpenStyle::MaxValue => "en yüksek puanla",
                        OpenStyle::MaxCover => "en çok taşla",
                        OpenStyle::Minimal => "en az taşla",
                        OpenStyle::SafeOkey => "okey seride değil",
                        OpenStyle::NoOkey => "okeyi elde tut",
                    };
                    format!("Perle aç ({how}{})", lay_note(lay))
                }
                OpenChoice::OpenPairs(lay) => format!("Çiftle aç{}", lay_note(lay)),
                OpenChoice::Lay(LayStyle::All) => "İndir / işle".to_string(),
                OpenChoice::Lay(LayStyle::AllSafe) => "İndir / işle (okeyi seriye koyma)".to_string(),
                OpenChoice::Lay(LayStyle::Nothing) => "Hiçbir şey indirme".to_string(),
                OpenChoice::Finish => "Bitir".to_string(),
            };
            match discard {
                Some(t) => format!("{what}, {} at", name(*t)),
                None => format!("{what} (el biter)"),
            }
        }
    }
}

pub(super) fn shape_short(sh: &Shape) -> String {
    let okey = if sh.jokers() > 0 { "+okey" } else { "" };
    match *sh {
        Shape::Run { color, lo, hi, .. } => format!("{}{}-{}{okey}", COLOR_SHORT[color as usize], lo, if hi == 14 { 1 } else { hi }),
        Shape::Group { num, .. } => format!("{num}×{}{okey}", sh.len()),
        Shape::Pair { tile, .. } if tile == JOKER => "okey çifti".to_string(),
        Shape::Pair { tile, joker } => format!("{} çift{}", short(tile), if joker { "+okey" } else { "" }),
        Shape::GostergePair { other, .. } => format!("gösterge+{}", short(other)),
    }
}

/// A one-line summary of an explicit move that names the melds, so different arrangements read differently.
pub fn move_label(mv: &Move, rules: &Rules) -> String {
    let mut melds = Vec::new();
    let mut value = 0;
    let mut laid = Vec::new();
    let (mut swaps, mut gifts) = (0, 0);
    for act in &mv.acts {
        match act {
            Act::Meld(s) => {
                melds.push(shape_short(s));
                value += s.open_value(rules);
            }
            Act::Layoff { tile, .. } => laid.push(short(*tile)),
            Act::Swap(_) => swaps += 1,
            Act::Gift { .. } => gifts += 1,
        }
    }
    let mut parts = Vec::new();
    if !melds.is_empty() {
        let total = if value > 0 { format!(" ({value})") } else { String::new() };
        parts.push(format!("İndir {}{total}", melds.join(", ")));
    }
    if swaps > 0 {
        parts.push("okey al".to_string());
    }
    if !laid.is_empty() {
        parts.push(format!("işle {}", laid.join(", ")));
    }
    if gifts > 0 {
        parts.push(format!("{gifts} çift işle"));
    }
    if parts.is_empty() {
        parts.push("İndirme yok".to_string());
    }
    format!("{} · {} at", parts.join(" · "), name(mv.discard))
}

/// The physical steps of an explicit move, in order, for showing to the player.
pub fn describe_move(view: &View, mv: &Move) -> Vec<Step> {
    let mut g = view.determinize(&mut Rng::new(3));
    g.log = Some(Vec::new());
    let _ = g.play_move(0, mv, view.taken);
    g.log.take().unwrap_or_default()
}

/// The physical steps seat 0 performs for `d`, in order, for showing to the player.
pub fn describe(view: &View, d: Decision) -> Vec<Step> {
    let Decision::Act { choice, discard } = d else { return Vec::new() };
    let mut sv = Solver::new(&view.rules);
    let mut g = view.determinize(&mut Rng::new(3));
    g.log = Some(Vec::new());
    if let Ok(None) = g.apply_choice(&mut sv, 0, choice, view.taken)
        && let Some(t) = discard.filter(|&t| g.players[0].hand[t as usize] > 0)
    {
        g.discard(0, t);
    }
    g.log.take().unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Out {
    pub tile: Tile,
    pub unseen: u8,
    pub open_value: i32,
    pub pairs: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HandReport {
    pub meld_req: i32,
    pub pair_req: u8,
    pub open_value: i32,
    pub melds: Vec<Vec<Tile>>,
    pub pairs: u8,
    pub outs: Vec<Out>,
    pub problems: Vec<String>,
}

pub fn hand_report(view: &View) -> HandReport {
    let mut sv = Solver::new(&view.rules);
    let (meld_req, pair_req) = view.requirements();
    let hand = counts_from(&view.hand);
    let g = view.determinize(&mut Rng::new(7));
    let gost = view.players[0].gosterge_bonus.then_some(view.gosterge);
    let plan = g.open_search(&mut sv, &hand, None).unwrap_or_default();
    let open_value = plan.open_value(&view.rules);
    let pairs = pair_plan(&hand, &view.rules, gost).pairs.len() as u8;
    let unseen = view.unseen();
    let mut outs = Vec::new();
    for t in 0..NIDS as Tile {
        if unseen[t as usize] == 0 {
            continue;
        }
        let mut h = hand;
        h[t as usize] += 1;
        let v = g.open_search(&mut sv, &h, None).map_or(0, |p| p.open_value(&view.rules));
        let pr = pair_plan(&h, &view.rules, gost).pairs.len() as u8;
        if v > open_value || pr > pairs {
            outs.push(Out {
                tile: t,
                unseen: unseen[t as usize],
                open_value: v,
                pairs: pr,
            });
        }
    }
    outs.sort_by_key(|o| std::cmp::Reverse(o.open_value));
    HandReport {
        meld_req,
        pair_req,
        open_value,
        melds: plan.melds.iter().map(Shape::tiles).collect(),
        pairs,
        outs,
        problems: view.problems(),
    }
}
