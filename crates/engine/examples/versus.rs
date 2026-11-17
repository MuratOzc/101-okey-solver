//! Players compared on the same deals at one table: the hero sits against three opponents of any kind.
//! Usage: versus <hands> <network> <opponents, comma separated> <hero>...
//! A seat is: finisher | tuned | basic | god | seer | model | app, with options after plus signs:
//!   model+n=<network>            the network alone
//!   app+<deals>+h<k>+blind+peek+n=<network>   search; h = turns of mine before the network's value is taken
//!                                (0 = every deal to the end), blind = no guessing of hands, peek = told every hand, peek1 = only the hand to my right (2 across, 3 left)
//! Opponents rotate around the table from hand to hand. With ",partners" after the opponents the seat
//! across is the hero's partner, nobody rotates, and the lead is team against team.

use okey_engine::agent::*;
use okey_engine::analysis::Options;
use okey_engine::game::{Game, Outcome};
use okey_engine::meld::Solver;
use okey_engine::model::{ModelAgent, play_out_with, turns};
use okey_engine::net::Net;
use okey_engine::policy::Policy;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const PLAY_ON_DISCARDS: usize = 5;

/// Sees every hand and the order of the deck, and knows how each other seat decides: bots by their
/// policy, the hero by the network. Tries drawing and taking, every plan and discard, against that truth.
struct Seer {
    policies: [Policy; 4],
    by_net: [bool; 4],
    net: Arc<Net>,
    discards: usize,
}

impl Agent for Seer {
    fn name(&self) -> String {
        "seer".into()
    }

    fn turn(&mut self, g: &mut Game, sv: &mut Solver, first: bool) -> Option<Outcome> {
        let p = g.current;
        g.policy = self.policies;
        if !first && g.deck.is_empty() {
            return Some(Outcome::DeckOut);
        }
        g.table.new_turn();
        let mut starts = Vec::new();
        if first {
            starts.push((g.clone(), None));
        } else {
            let mut d = g.clone();
            if d.draw(p).is_some() {
                starts.push((d, None));
            }
            if let Some(t) = g.top.filter(|&t| g.can_use_taken(sv, p, t)) {
                let mut h = g.clone();
                h.take_top(p);
                starts.push((h, Some(t)));
            }
        }
        let mut best: Option<(i32, Game, Option<Outcome>)> = None;
        for (start, taken) in &starts {
            for t in turns(start, sv, p, *taken, self.discards) {
                let cost = match &t.end {
                    Some(end) => t.after.gap3(&t.after.scores(end), p),
                    None => {
                        let mut sim = t.after.clone();
                        let end = play_out_with(&mut sim, sv, &self.net, self.by_net);
                        sim.gap3(&sim.scores(&end), p)
                    }
                };
                if best.as_ref().is_none_or(|b| cost < b.0) {
                    best = Some((cost, t.after, t.end));
                }
            }
        }
        match best {
            Some((_, after, end)) => {
                *g = after;
                end
            }
            None => {
                let (start, taken) = starts.swap_remove(0);
                *g = start;
                let end = g.bot_act(sv, p, taken);
                if end.is_none() {
                    g.current = (p + 1) % 4;
                }
                end
            }
        }
    }
}

fn kind(spec: &str) -> &str {
    spec.split('+').next().unwrap()
}

fn policy_of(spec: &str) -> Policy {
    match kind(spec) {
        "finisher" => Policy::FINISHER,
        "holder" => Policy::HOLDER,
        "basic" => Policy::BASIC,
        _ => Policy::TUNED,
    }
}

struct Nets {
    default: String,
    loaded: HashMap<String, Arc<Net>>,
}

impl Nets {
    fn of(&self, spec: &str) -> Arc<Net> {
        let path = spec.split('+').find_map(|o| o.strip_prefix("n=")).unwrap_or(&self.default);
        self.loaded[path].clone()
    }
}

fn seat(spec: &str, nets: &Nets, seed: u64, table: [&str; 4]) -> Box<dyn Agent> {
    let bot = |policy| -> Box<dyn Agent> {
        Box::new(BotAgent {
            label: spec.into(),
            policy,
        })
    };
    let mut parts = spec.split('+');
    match parts.next().unwrap() {
        "finisher" | "holder" | "tuned" | "basic" => bot(policy_of(spec)),
        "god" => Box::new(OracleAgent {
            label: spec.into(),
            policy: Policy::TUNED,
            depth: 0,
        }),
        "seer" => Box::new(Seer {
            policies: table.map(policy_of),
            by_net: table.map(|k| matches!(kind(k), "model" | "app")),
            net: nets.of(spec),
            discards: 8,
        }),
        "model" => Box::new(ModelAgent {
            label: spec.into(),
            net: nets.of(spec),
        }),
        "app" => {
            let mut opts = Options::default();
            let mut iterations = 80;
            let mut peek = [false; 4];
            for o in parts {
                if let Some(seats) = o.strip_prefix("peek") {
                    for p in 1..4 {
                        peek[p] = seats.is_empty() || seats.contains(char::from(b'0' + p as u8));
                    }
                } else if o == "blind" {
                    opts.infer = false;
                } else if o == "hold" {
                    opts.policy = Policy::HOLDER;
                } else if o == "built" {
                    opts.build = true;
                } else if let Some(h) = o.strip_prefix('h') {
                    opts.horizon = h.parse().expect("horizon");
                } else if !o.starts_with("n=") {
                    iterations = o.parse().expect("deals");
                }
            }
            Box::new(HelpedAgent {
                label: spec.into(),
                opts,
                iterations,
                seed,
                help: Help {
                    peek,
                    model: Some((nets.of(spec), 0.0, true)),
                },
            })
        }
        other => panic!("unknown seat {other}"),
    }
}

fn spread(v: &[f64]) -> (f64, f64) {
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    (mean, (var / n).sqrt())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let hands: u64 = args[1].parse().expect("hands");
    let (seats, partners) = match args[3].strip_suffix(",partners") {
        Some(seats) => (seats, true),
        None => (args[3].as_str(), false),
    };
    let table: Vec<String> = seats.split(',').map(String::from).collect();
    assert_eq!(table.len(), 3, "three other seats");
    let heroes: Vec<String> = args[4..].to_vec();
    let mut nets = Nets {
        default: args[2].clone(),
        loaded: HashMap::new(),
    };
    for spec in heroes.iter().chain(&table).map(String::as_str).chain(["model"]) {
        let path = spec
            .split('+')
            .find_map(|o| o.strip_prefix("n="))
            .unwrap_or(&nets.default)
            .to_string();
        nets.loaded.entry(path.clone()).or_insert_with(|| {
            let mut net = Net::from_file(&path).expect("network");
            net.discards = PLAY_ON_DISCARDS;
            Arc::new(net)
        });
    }
    let rules = Rules {
        partners,
        ..Rules::default()
    };
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(2).max(1));
    let next = AtomicU64::new(0);
    let start = Instant::now();
    let mut rows: Vec<(u64, Vec<[i32; 4]>, usize, Vec<f64>)> = std::thread::scope(|scope| {
        let ws: Vec<_> = (0..threads)
            .map(|_| {
                let (next, rules, nets, table, heroes) = (&next, &rules, &nets, &table, &heroes);
                scope.spawn(move || {
                    let mut sv = Solver::new(rules);
                    let mut out = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= hands {
                            break;
                        }
                        let mut rng = Rng::new(0xD0E1 ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
                        let deal = Game::deal(rules, &mut rng, (i % 4) as u8);
                        let rot = if partners { 0 } else { (i / 4 % 3) as usize };
                        let (mut per, mut took) = (Vec::new(), Vec::new());
                        for h in heroes {
                            let mut g = deal.clone();
                            let kinds: [&str; 4] =
                                std::array::from_fn(|q| if q == 0 { h.as_str() } else { table[(q - 1 + rot) % 3].as_str() });
                            let mut hero = seat(h, nets, i, kinds);
                            let mut o: Vec<Box<dyn Agent>> = (1..4).map(|q| seat(kinds[q], nets, i * 7 + q as u64, kinds)).collect();
                            let [a, b, c] = &mut o[..] else { unreachable!() };
                            let t = Instant::now();
                            per.push(play_hand(&mut g, &mut sv, &mut [hero.as_mut(), a.as_mut(), b.as_mut(), c.as_mut()]));
                            took.push(t.elapsed().as_secs_f64());
                        }
                        out.push((i, per, rot, took));
                    }
                    out
                })
            })
            .collect();
        ws.into_iter().flat_map(|w| w.join().unwrap()).collect()
    });
    rows.sort_by_key(|r| r.0);
    println!("{hands} hands against {table:?}, {:.0}s", start.elapsed().as_secs_f64());
    println!(
        "{:<28}{:>16}{:>20}{:>10}{:>10}{:>14}{:>10}  against each kind",
        "hero",
        "lead per hand",
        format!("over {}", heroes[0]),
        "finished",
        "best of 4",
        "matches won",
        "s/hand"
    );
    let lead_of = |r: &(u64, Vec<[i32; 4]>, usize, Vec<f64>), k: usize| {
        let s = r.1[k];
        if partners {
            (s[1] + s[3] - s[0] - s[2]) as f64
        } else {
            (s[1] + s[2] + s[3]) as f64 / 3.0 - s[0] as f64
        }
    };
    for (k, h) in heroes.iter().enumerate() {
        let lead: Vec<f64> = rows.iter().map(|r| lead_of(r, k)).collect();
        let over: Vec<f64> = rows.iter().map(|r| lead_of(r, k) - lead_of(r, 0)).collect();
        let fin = rows.iter().filter(|r| r.1[k][0] <= -101).count() as f64 / rows.len() as f64;
        let best: f64 = rows
            .iter()
            .map(|r| {
                let s = r.1[k];
                let m = *s.iter().min().unwrap();
                if s[0] == m {
                    1.0 / s.iter().filter(|&&x| x == m).count() as f64
                } else {
                    0.0
                }
            })
            .sum::<f64>()
            / rows.len() as f64;
        let mut rng = Rng::new(99);
        let (mut won, trials) = (0.0, 40000);
        for _ in 0..trials {
            let mut tot = [0i32; 4];
            for _ in 0..11 {
                let s = rows[rng.below(rows.len())].1[k];
                for q in 0..4 {
                    tot[q] += s[q];
                }
            }
            if partners {
                tot = [tot[0] + tot[2], tot[1] + tot[3], i32::MAX, i32::MAX];
            }
            let m = tot[1].min(tot[2]).min(tot[3]);
            if tot[0] < m {
                won += 1.0;
            } else if tot[0] == m {
                won += 0.5;
            }
        }
        let p = won / trials as f64;
        let pse = (p * (1.0 - p) / (rows.len() as f64 / 11.0)).sqrt();
        let mut names: Vec<&str> = table.iter().map(|s| s.as_str()).collect();
        names.sort();
        names.dedup();
        let mut kinds: Vec<String> = Vec::new();
        for name in names {
            let mut d: Vec<f64> = Vec::new();
            for r in &rows {
                let s = r.1[k];
                for q in 0..3 {
                    if table[(q + r.2) % 3] == name {
                        d.push((s[q + 1] - s[0]) as f64);
                    }
                }
            }
            let (m, se) = spread(&d);
            kinds.push(format!("{name} {m:+.1}±{se:.1}"));
        }
        let ((l, lse), (o, ose)) = (spread(&lead), spread(&over));
        let secs = rows.iter().map(|r| r.3[k]).sum::<f64>() / rows.len() as f64;
        println!(
            "{h:<28}{l:>+9.1} ± {lse:<4.1}{o:>+13.1} ± {ose:<4.1}{:>9.0}%{:>9.0}%{:>9.0}% ± {:<2.0}{secs:>10.3}  {}",
            fin * 100.0,
            best * 100.0,
            p * 100.0,
            pse * 100.0,
            kinds.join("  ")
        );
    }
    println!();
}
