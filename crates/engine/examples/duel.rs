use okey_engine::agent::*;
use okey_engine::analysis::Options;
use okey_engine::game::Game;
use okey_engine::meld::Solver;
use okey_engine::model::ModelAgent;
use okey_engine::net::Net;
use okey_engine::policy::Policy;
use okey_engine::rng::Rng;
use okey_engine::rules::Rules;
use okey_engine::tile::NIDS;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const USAGE: &str = "duel <hands> <opponents: tuned | basic | mixed; add \",partners\" for teams, the gap is then team against team> <player>...\nplayers: tuned | basic | oracle | search:<iterations> | plans:<iterations> | peek:<iterations> | model:<network> | hybrid:<iterations>:<network's weight in deals>:<network plays my later turns, 0 or 1>:<network>";

const PLAY_ON_DISCARDS: usize = 5;

fn player(spec: &str, nets: &[(String, Arc<Net>)], seed: u64) -> Box<dyn Agent> {
    let bot = |policy| -> Box<dyn Agent> {
        Box::new(BotAgent {
            label: spec.into(),
            policy,
        })
    };
    match spec.split_once(':') {
        None if spec == "tuned" => bot(Policy::TUNED),
        None if spec == "basic" => bot(Policy::BASIC),
        None if spec == "oracle" => Box::new(OracleAgent {
            label: spec.into(),
            policy: Policy::TUNED,
            depth: 0,
        }),
        Some(("search", n)) => Box::new(SearchAgent {
            label: spec.into(),
            opts: Options::default(),
            iterations: n.parse().expect("iterations"),
            seed,
        }),
        Some(("searchf", n)) => Box::new(SearchAgent {
            label: spec.into(),
            opts: Options {
                policy: Policy::FINISHER,
                ..Options::default()
            },
            iterations: n.parse().expect("iterations"),
            seed,
        }),
        Some(("plans", n)) => Box::new(SearchAgent {
            label: spec.into(),
            opts: Options {
                exhaustive: false,
                max_discards: 53,
                ..Options::default()
            },
            iterations: n.parse().expect("iterations"),
            seed,
        }),
        Some(("peek", n)) => Box::new(HelpedAgent {
            label: spec.into(),
            opts: Options::default(),
            iterations: n.parse().expect("iterations"),
            seed,
            help: Help {
                peek: [false, true, true, true],
                model: None,
            },
        }),
        Some(("hybrid", rest)) => {
            let part: Vec<&str> = rest.splitn(4, ':').collect();
            let key = format!("{PLAY_ON_DISCARDS}:{}", part[3]);
            let net = nets.iter().find(|(p, _)| *p == key).expect("network").1.clone();
            Box::new(HelpedAgent {
                label: spec.into(),
                opts: Options::default(),
                iterations: part[0].parse().expect("iterations"),
                seed,
                help: Help {
                    peek: [false; 4],
                    model: Some((net, part[1].parse().expect("weight"), part[2] == "1")),
                },
            })
        }
        Some(("model", path)) => Box::new(ModelAgent {
            label: spec.into(),
            net: nets
                .iter()
                .find(|(p, _)| *p == format!("{NIDS}:{path}"))
                .expect("network")
                .1
                .clone(),
        }),
        _ => panic!("unknown player {spec}"),
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
    let hands: u64 = args.get(1).and_then(|s| s.parse().ok()).expect(USAGE);
    let versus = args.get(2).expect(USAGE).clone();
    let (versus, partners) = match versus.strip_suffix(",partners") {
        Some(v) => (v.to_string(), true),
        None => (versus, false),
    };
    let specs: Vec<String> = args[3..].to_vec();
    let nets: Vec<(String, Arc<Net>)> = specs
        .iter()
        .filter_map(|s| {
            let alone = s.strip_prefix("model:").map(|p| (p, NIDS));
            alone.or_else(|| {
                s.strip_prefix("hybrid:")
                    .and_then(|r| r.splitn(4, ':').nth(3))
                    .map(|p| (p, PLAY_ON_DISCARDS))
            })
        })
        .map(|(p, discards)| {
            let mut net = Net::from_file(p).expect("network");
            net.discards = discards;
            (format!("{discards}:{p}"), Arc::new(net))
        })
        .collect();
    let rules = Rules {
        partners,
        ..Rules::default()
    };
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(2).max(1));
    let next = AtomicU64::new(0);
    let start = Instant::now();
    let mut rows: Vec<Vec<f64>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                let (next, rules, specs, nets, versus) = (&next, &rules, &specs, &nets, &versus);
                scope.spawn(move || {
                    let mut sv = Solver::new(rules);
                    let mut rows = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= hands {
                            break;
                        }
                        let mut rng = Rng::new(0xD0E1 ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
                        let deal = Game::deal(rules, &mut rng, (i % 4) as u8);
                        let rivals: Vec<Policy> = (0..3)
                            .map(|k| match versus.as_str() {
                                "basic" => Policy::BASIC,
                                "finisher" => Policy::FINISHER,
                                "mixed" if (i as usize + k).is_multiple_of(2) => Policy::BASIC,
                                _ => Policy::TUNED,
                            })
                            .collect();
                        let mut row = vec![i as f64];
                        let mut raw = Vec::new();
                        for spec in specs {
                            let mut g = deal.clone();
                            let mut hero = player(spec, nets, i);
                            let mut others: Vec<BotAgent> = rivals
                                .iter()
                                .map(|&policy| BotAgent {
                                    label: String::new(),
                                    policy,
                                })
                                .collect();
                            let [a, b, c] = &mut others[..] else { unreachable!() };
                            let s = play_hand(&mut g, &mut sv, &mut [hero.as_mut(), a, b, c]);
                            raw.extend(s.map(|x| x as f64));
                            row.push(if partners {
                                (s[0] + s[2] - s[1] - s[3]) as f64
                            } else {
                                s[0] as f64 - (s[1] + s[2] + s[3]) as f64 / 3.0
                            });
                        }
                        row.extend(raw);
                        rows.push(row);
                    }
                    rows
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().unwrap()).collect()
    });
    rows.sort_by(|a, b| a[0].total_cmp(&b[0]));
    println!("{hands} hands against {versus} bots, {:.0}s\n", start.elapsed().as_secs_f64());
    println!("{:<34}{:>16}{:>24}", "player", "gap per hand", format!("better than {}", specs[0]));
    for (k, spec) in specs.iter().enumerate() {
        let own: Vec<f64> = rows.iter().map(|r| r[k + 1]).collect();
        let edge: Vec<f64> = rows.iter().map(|r| r[1] - r[k + 1]).collect();
        let ((m, se), (dm, dse)) = (spread(&own), spread(&edge));
        println!("{spec:<34}{:>9.1} ± {:<4.1}{:>17.1} ± {:<4.1}", m, se, dm, dse);
    }
    let Some(len) = std::env::var("DUEL_MATCH").ok().and_then(|v| v.parse::<usize>().ok()) else {
        return;
    };
    let scores = |r: &Vec<f64>, k: usize| -> [f64; 4] { std::array::from_fn(|q| r[1 + specs.len() + 4 * k + q]) };
    let games: Vec<&[Vec<f64>]> = rows.chunks_exact(len).collect();
    println!(
        "
{} games of {len} hands; totals are summed scores, lower is better
",
        games.len()
    );
    println!(
        "{:<34}{:>22}{:>12}{:>16}",
        "player", "my lead after game", "games won", "ahead of all"
    );
    for (k, spec) in specs.iter().enumerate() {
        let (mut leads, mut won, mut ahead) = (Vec::new(), 0.0, 0.0);
        for game in &games {
            let mut tot = [0.0; 4];
            for r in game.iter() {
                for (t, x) in tot.iter_mut().zip(scores(r, k)) {
                    *t += x;
                }
            }
            let rest = (tot[1] + tot[2] + tot[3]) / 3.0;
            leads.push(rest - tot[0]);
            let best_other = tot[1].min(tot[2]).min(tot[3]);
            if tot[0] < best_other {
                won += 1.0;
            } else if tot[0] == best_other {
                won += 0.5;
            }
            if tot[0] < tot[1] && tot[0] < tot[2] && tot[0] < tot[3] {
                ahead += 1.0;
            }
        }
        let (m, se) = spread(&leads);
        let n = games.len() as f64;
        println!(
            "{spec:<34}{:>13.0} ± {:<6.0}{:>11.0}%{:>15.0}%",
            m,
            se,
            100.0 * won / n,
            100.0 * ahead / n
        );
    }
    if let Some(game) = games.first() {
        for (k, spec) in specs.iter().enumerate() {
            println!(
                "
first game, {spec}: hand, then scores me / right / across / left, running totals"
            );
            let mut tot = [0.0; 4];
            for (h, r) in game.iter().enumerate() {
                let s = scores(r, k);
                for (t, x) in tot.iter_mut().zip(s) {
                    *t += x;
                }
                println!(
                    "  {:>2}: {:>5} {:>5} {:>5} {:>5}   | {:>6} {:>6} {:>6} {:>6}",
                    h + 1,
                    s[0],
                    s[1],
                    s[2],
                    s[3],
                    tot[0],
                    tot[1],
                    tot[2],
                    tot[3]
                );
            }
        }
    }
}
