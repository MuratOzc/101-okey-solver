//! A network that starts from nothing plays all four seats against itself, choosing among every legal
//! move, and writes down each position it moved to with how that hand ended for the mover.
//! Usage: selfplay play <hands> <out dir> <network | none> <random moves per cent> <seed> [threads]
//!        selfplay test <hands> <network> [threads]     the network alone against three finisher bots
//! Without a network every move is drawn at random from the legal ones.

use okey_engine::features::{Feat, LAYOUT, Moment, NBITS, NREAL, WORDS, encode};
use okey_engine::game::{Game, Outcome};
use okey_engine::meld::Solver;
use okey_engine::model::{draw_or_take, model_act_all, model_turn_all, written3};
use okey_engine::moves::{Move, generate};
use okey_engine::net::Net;
use okey_engine::policy::Policy;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use okey_engine::tile::*;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

// Must match gen_data.rs and train/train.py.
const Y_SCALE: f32 = 300.0;

struct Seen {
    seat: u8,
    feat: Feat,
    written: i32,
    draw: bool,
}

fn chance(rng: &mut Rng, per_cent: usize) -> bool {
    rng.below(100) < per_cent
}

/// Any legal move, each position and then each tile of it as likely as the next.
fn random_act(g: &mut Game, rng: &mut Rng, taken: Option<Tile>) -> Option<Option<Outcome>> {
    let p = g.current;
    let found = generate(g, p, taken, usize::MAX).positions;
    if found.is_empty() {
        return None;
    }
    for _ in 0..64 {
        let acts = &found[rng.below(found.len())];
        let mut a = g.clone();
        if a.play_acts(p, acts, taken).is_err() {
            continue;
        }
        let hand = a.players[p as usize].hand;
        let tiles: Vec<Tile> = (0..NIDS as Tile).filter(|&t| hand[t as usize] > 0).collect();
        if tiles.is_empty() {
            continue;
        }
        let mv = Move {
            acts: acts.clone(),
            discard: tiles[rng.below(tiles.len())],
        };
        let mut b = g.clone();
        if let Ok(end) = b.play_move(p, &mv, taken) {
            if end.is_none() {
                b.current = (p + 1) % 4;
            }
            *g = b;
            return Some(end);
        }
    }
    None
}

fn turn(
    g: &mut Game,
    sv: &mut Solver,
    net: Option<&Net>,
    wild: usize,
    rng: &mut Rng,
    first: bool,
    seen: &mut Vec<Seen>,
) -> Option<Outcome> {
    let p = g.current;
    if !first && g.deck.is_empty() {
        return Some(Outcome::DeckOut);
    }
    g.table.new_turn();
    let full = net.is_some_and(|n| n.full());
    let mut taken = None;
    if !first {
        let offered = g.top.filter(|&t| g.can_use_taken(sv, p, t));
        let take = match (offered, net) {
            (None, _) => false,
            (Some(_), Some(net)) if !chance(rng, wild) => draw_or_take(net, g, sv, p).is_some_and(|(draw, take)| take < draw),
            (Some(_), _) => chance(rng, 50),
        };
        if take {
            let t = g.take_top(p);
            seen.push(Seen {
                seat: p,
                feat: encode(g, sv, p, Moment::Take(t), full),
                written: written3(g, p),
                draw: true,
            });
            taken = Some(t);
        } else {
            if offered.is_some() {
                seen.push(Seen {
                    seat: p,
                    feat: encode(g, sv, p, Moment::Draw, full),
                    written: written3(g, p),
                    draw: true,
                });
            }
            g.draw(p)?;
        }
    }
    let end = match net {
        Some(net) if !chance(rng, wild) => model_act_all(g, sv, net, taken).0,
        _ => match random_act(g, rng, taken) {
            Some(end) => end,
            None => {
                let end = g.bot_act(sv, p, taken);
                if end.is_none() {
                    g.current = (p + 1) % 4;
                }
                end
            }
        },
    };
    if end.is_none() {
        seen.push(Seen {
            seat: p,
            feat: encode(g, sv, p, Moment::After, full),
            written: written3(g, p),
            draw: false,
        });
    }
    end
}

fn threads_from(arg: Option<&String>) -> usize {
    arg.and_then(|s| s.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()))
}

fn play(args: &[String]) {
    let hands: u64 = args[0].parse().expect("hands");
    let dir = &args[1];
    let net = (args[2] != "none").then(|| Net::from_file(&args[2]).expect("network"));
    let wild: usize = args[3].parse().expect("random moves per cent");
    let seed: u64 = args[4].parse().expect("seed");
    let threads = threads_from(args.get(5));
    std::fs::create_dir_all(dir).expect("out dir");
    let rules = Rules::default();
    let (next, records) = (AtomicU64::new(0), AtomicU64::new(0));
    let size = WORDS * 8 + NREAL + 5;
    let start = Instant::now();
    std::thread::scope(|scope| {
        for id in 0..threads {
            let (next, records, rules, net) = (&next, &records, &rules, net.as_ref());
            scope.spawn(move || {
                let file = std::fs::File::create(format!("{dir}/shard-{seed}-{id:02}.bin")).expect("shard");
                let mut file = BufWriter::with_capacity(1 << 20, file);
                file.write_all(b"OKDT").unwrap();
                for x in [LAYOUT, NBITS as u32, NREAL as u32, size as u32, 0] {
                    file.write_all(&x.to_le_bytes()).unwrap();
                }
                let mut sv = Solver::new(rules);
                let (mut seen, mut out) = (Vec::new(), Vec::new());
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= hands {
                        break;
                    }
                    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ i);
                    let mut g = Game::deal(rules, &mut rng, (i % 4) as u8);
                    seen.clear();
                    let mut end = None;
                    let mut first = true;
                    for _ in 0..400 {
                        end = turn(&mut g, &mut sv, net, wild, &mut rng, first, &mut seen);
                        first = false;
                        if end.is_some() {
                            break;
                        }
                    }
                    let scores = g.scores(&end.unwrap_or(Outcome::DeckOut));
                    out.clear();
                    for s in &seen {
                        let y = (g.gap3(&scores, s.seat) - s.written) as f32 / Y_SCALE;
                        for w in s.feat.bits {
                            out.extend_from_slice(&w.to_le_bytes());
                        }
                        out.extend_from_slice(&s.feat.real);
                        out.extend_from_slice(&y.to_le_bytes());
                        out.push(1 | (s.draw as u8) << 1);
                    }
                    records.fetch_add(seen.len() as u64, Ordering::Relaxed);
                    file.write_all(&out).unwrap();
                }
            });
        }
    });
    let s = start.elapsed().as_secs_f64();
    println!(
        "{hands} hands, {} positions, {s:.0} s, {:.1} hands/s",
        records.load(Ordering::Relaxed),
        hands as f64 / s
    );
}

fn test(args: &[String]) {
    let hands: u64 = args[0].parse().expect("hands");
    let net = Net::from_file(&args[1]).expect("network");
    let threads = threads_from(args.get(2));
    let rules = Rules::default();
    let next = AtomicU64::new(0);
    let leads: Vec<f64> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let (next, rules, net) = (&next, &rules, &net);
                scope.spawn(move || {
                    let mut sv = Solver::new(rules);
                    let mut leads = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= hands {
                            break;
                        }
                        let mut g = Game::deal(rules, &mut Rng::new(i * 131 + 3), (i % 4) as u8);
                        g.policy = [Policy::FINISHER; 4];
                        let mut first = true;
                        let mut end = None;
                        for _ in 0..400 {
                            end = if g.current == 0 {
                                model_turn_all(&mut g, &mut sv, net, first)
                            } else if first {
                                g.starter_turn(&mut sv)
                            } else {
                                g.bot_turn(&mut sv)
                            };
                            first = false;
                            if end.is_some() {
                                break;
                            }
                        }
                        let scores = g.scores(&end.unwrap_or(Outcome::DeckOut));
                        leads.push(-(g.gap3(&scores, 0) as f64) / 3.0);
                    }
                    leads
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    let n = leads.len() as f64;
    let mean = leads.iter().sum::<f64>() / n;
    let se = (leads.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0) / n).sqrt();
    println!(
        "lead per hand against three finisher bots: {mean:+.1} ± {:.1} ({hands} hands)",
        1.96 * se
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("play") if args.len() >= 7 => play(&args[2..]),
        Some("test") if args.len() >= 4 => test(&args[2..]),
        _ => eprintln!(
            "usage: selfplay play <hands> <out dir> <network | none> <random moves per cent> <seed> [threads]\n       selfplay test <hands> <network> [threads]"
        ),
    }
}
