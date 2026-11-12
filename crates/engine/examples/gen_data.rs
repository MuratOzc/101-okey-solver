use okey_engine::features::{Feat, LAYOUT, Moment, NBITS, NREAL, encode};
use okey_engine::game::{Game, Outcome};
use okey_engine::meld::Solver;
use okey_engine::model::{draw_or_take, model_act, play_out_with, turn_value, turns, written3};
use okey_engine::net::Net;
use okey_engine::policy::Policy;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const USAGE: &str = "gen_data <out-dir> <hands> [seed] [network | -] [network seats %] [options kept] [wild %] [deck orders] [network plays on: 1 | 0] [discards it weighs when playing on] [partners: 1 | 0]";
const Y_SCALE: f32 = 300.0;

struct Setup<'a> {
    net: Option<&'a Net>,
    keep: usize,
    wild: usize,
    decks: u64,
    carry_on: bool,
}

fn redealt(g: &Game, seed: u64, k: u64) -> Game {
    let mut g = g.clone();
    if k > 0 {
        Rng::new(seed ^ k.wrapping_mul(0xA24B_AED4_963E_E407)).shuffle(&mut g.deck);
    }
    g
}

fn record(out: &mut Vec<u8>, f: &Feat, y: f32, first: bool, draw: bool) {
    for w in f.bits {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out.extend_from_slice(&f.real);
    out.extend_from_slice(&(y / Y_SCALE).to_le_bytes());
    out.push(first as u8 | (draw as u8) << 1);
}

fn play_on(mut g: Game, sv: &mut Solver, s: &Setup, me: u8) -> i32 {
    let end = match s.net.filter(|_| s.carry_on) {
        Some(net) => {
            let mut seats = [false; 4];
            seats[me as usize] = true;
            play_out_with(&mut g, sv, net, seats)
        }
        None => {
            g.policy[me as usize] = Policy::TUNED;
            g.play_out(sv)
        }
    };
    g.gap3(&g.scores(&end), me)
}

fn act_and_play_on(mut g: Game, sv: &mut Solver, s: &Setup, me: u8, taken: Option<u8>) -> i32 {
    let end = match s.net.filter(|_| s.carry_on) {
        Some(net) => model_act(&mut g, sv, net, taken),
        None => {
            g.policy[me as usize] = Policy::TUNED;
            let end = g.bot_act(sv, me, taken);
            if end.is_none() {
                g.current = (me + 1) % 4;
            }
            end
        }
    };
    match end {
        Some(end) => g.gap3(&g.scores(&end), me),
        None => play_on(g, sv, s, me),
    }
}

fn recorded_turn(
    g: &mut Game,
    sv: &mut Solver,
    s: &Setup,
    model: [bool; 4],
    rng: &mut Rng,
    first: bool,
    out: &mut Vec<u8>,
) -> Option<Outcome> {
    let p = g.current;
    let seat_is_model = model[p as usize] && s.net.is_some();
    g.table.new_turn();
    let mut taken = None;
    if !first {
        let mut take = false;
        if let Some(t) = g.top.filter(|&t| g.can_use_taken(sv, p, t)) {
            if !g.deck.is_empty() {
                let seed = rng.next_u64();
                let mut took = g.clone();
                took.take_top(p);
                let (mut y_draw, mut y_take) = (0i64, 0i64);
                for k in 0..s.decks {
                    let mut drawn = redealt(g, seed, k);
                    let x = drawn.deck.pop().unwrap();
                    drawn.players[p as usize].hand[x as usize] += 1;
                    y_draw += (act_and_play_on(drawn, sv, s, p, None) - written3(g, p)) as i64;
                    y_take += (act_and_play_on(redealt(&took, seed, k), sv, s, p, Some(t)) - written3(&took, p)) as i64;
                }
                record(
                    out,
                    &encode(g, sv, p, Moment::Draw, true),
                    y_draw as f32 / s.decks as f32,
                    true,
                    true,
                );
                record(
                    out,
                    &encode(&took, sv, p, Moment::Take(t), true),
                    y_take as f32 / s.decks as f32,
                    false,
                    true,
                );
            }
            take = if rng.below(100) < s.wild {
                rng.below(2) == 0
            } else if seat_is_model {
                draw_or_take(s.net.unwrap(), g, sv, p).is_some_and(|(d, t)| t < d)
            } else {
                g.bot_wants_take(sv, p)
            };
        }
        if take {
            taken = Some(g.take_top(p));
        } else {
            let Some(t) = g.deck.pop() else { return Some(Outcome::DeckOut) };
            g.players[p as usize].hand[t as usize] += 1;
        }
    }

    let options = turns(g, sv, p, taken, okey_engine::tile::NIDS);
    let values: Vec<f32> = match s.net {
        Some(net) => options.iter().map(|t| turn_value(net, sv, t, p)).collect(),
        None => vec![],
    };
    let mut open: Vec<usize> = (0..options.len()).filter(|&i| options[i].end.is_none()).collect();
    rng.shuffle(&mut open);
    let mut picked: Vec<usize> = Vec::new();
    if s.net.is_some() {
        let mut ranked = open.clone();
        ranked.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
        picked.extend(ranked.into_iter().take(s.keep / 2));
    } else {
        let mut b = g.clone();
        b.policy[p as usize] = Policy::TUNED;
        if b.bot_act(sv, p, taken).is_none() {
            let same = |i: &usize| {
                options[*i].after.table.melds.len() == b.table.melds.len()
                    && options[*i].after.players[p as usize].opened == b.players[p as usize].opened
            };
            if let Some(&i) = open.iter().find(|i| same(i) && options[**i].after.top == b.top) {
                picked.push(i);
            }
            let more: Vec<usize> = open
                .iter()
                .copied()
                .filter(|i| same(i) && !picked.contains(i))
                .take((s.keep / 2).saturating_sub(picked.len()))
                .collect();
            picked.extend(more);
        }
    }
    for &i in &open {
        if picked.len() >= s.keep {
            break;
        }
        if !picked.contains(&i) {
            picked.push(i);
        }
    }
    if picked.len() >= 2 {
        let seed = rng.next_u64();
        for (n, &i) in picked.iter().enumerate() {
            let after = &options[i].after;
            let y: i64 = (0..s.decks)
                .map(|k| (play_on(redealt(after, seed, k), sv, s, p) - written3(after, p)) as i64)
                .sum();
            record(
                out,
                &encode(after, sv, p, Moment::After, true),
                y as f32 / s.decks as f32,
                n == 0,
                false,
            );
        }
    }

    let choice = if options.is_empty() {
        None
    } else if rng.below(100) < s.wild {
        Some(rng.below(options.len()))
    } else if seat_is_model {
        (0..options.len()).min_by(|&a, &b| values[a].total_cmp(&values[b]))
    } else {
        None
    };
    match choice {
        Some(i) => {
            let t = options.into_iter().nth(i).unwrap();
            *g = t.after;
            t.end
        }
        None => {
            let end = g.bot_act(sv, p, taken);
            if end.is_none() {
                g.current = (p + 1) % 4;
            }
            end
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = args.get(1).expect(USAGE).clone();
    let hands: u64 = args.get(2).and_then(|s| s.parse().ok()).expect(USAGE);
    let seed: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut net = args
        .get(4)
        .filter(|s| s.as_str() != "-")
        .map(|p| Net::from_file(p).expect("network"));
    if let Some(net) = &mut net {
        net.discards = args.get(10).and_then(|s| s.parse().ok()).unwrap_or(5);
    }
    let share: usize = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(50);
    let keep: usize = args.get(6).and_then(|s| s.parse().ok()).unwrap_or(6);
    let wild: usize = args.get(7).and_then(|s| s.parse().ok()).unwrap_or(10);
    let decks: u64 = args.get(8).and_then(|s| s.parse().ok()).unwrap_or(1).max(1);
    let carry_on = args.get(9).is_none_or(|s| s != "0");
    std::fs::create_dir_all(&dir).expect("out-dir");
    let partners = args.get(11).is_some_and(|s| s == "1");
    let rules = Rules {
        partners,
        ..Rules::default()
    };
    assert!(
        net.as_ref().is_none_or(|net| net.partners == partners),
        "the network was trained for the other kind of game"
    );
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(2).max(1));
    let next = AtomicU64::new(0);
    let records = AtomicU64::new(0);
    let start = Instant::now();
    let size = okey_engine::features::WORDS * 8 + NREAL + 5;
    std::thread::scope(|scope| {
        for id in 0..threads {
            let (next, records, rules, net, dir) = (&next, &records, &rules, net.as_ref(), &dir);
            scope.spawn(move || {
                let file = std::fs::File::create(format!("{dir}/shard-{seed}-{id:02}.bin")).expect("shard");
                let mut file = BufWriter::with_capacity(1 << 20, file);
                file.write_all(b"OKDT").unwrap();
                for x in [LAYOUT, NBITS as u32, NREAL as u32, size as u32, partners as u32] {
                    file.write_all(&x.to_le_bytes()).unwrap();
                }
                let setup = Setup {
                    net,
                    keep,
                    wild,
                    decks,
                    carry_on,
                };
                let mut sv = Solver::new(rules);
                let mut out = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= hands {
                        break;
                    }
                    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ i);
                    let mut g = Game::deal(rules, &mut rng, (i % 4) as u8);
                    let mut model = [false; 4];
                    for p in 0..4 {
                        g.policy[p] = Policy::varied(&mut rng);
                        model[p] = rng.below(100) < share;
                    }
                    out.clear();
                    let mut first = true;
                    for _ in 0..400 {
                        if recorded_turn(&mut g, &mut sv, &setup, model, &mut rng, first, &mut out).is_some() {
                            break;
                        }
                        first = false;
                    }
                    records.fetch_add((out.len() / size) as u64, Ordering::Relaxed);
                    file.write_all(&out).unwrap();
                }
                file.flush().unwrap();
            });
        }
    });
    let n = records.load(Ordering::Relaxed);
    println!(
        "{hands} hands, {n} positions, {:.1} GB, {:.0}s on {threads} threads",
        (n * size as u64) as f64 / 1e9,
        start.elapsed().as_secs_f64()
    );
}
