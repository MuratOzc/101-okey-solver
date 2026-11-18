//! What the measuring tools share.
#![allow(dead_code)]

use okey_engine::meld::Solver;
use okey_engine::rules::Rules;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Leaves two cores for the rest of the machine.
pub fn threads() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(2).max(1))
}

/// `job(sv, i)` for every `i` below `n`, spread over the cores; the results in the order of `i`.
pub fn parallel<T: Send>(rules: &Rules, n: u64, job: impl Fn(&mut Solver, u64) -> T + Sync) -> Vec<T> {
    let next = AtomicU64::new(0);
    let done = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads() {
            scope.spawn(|| {
                let mut sv = Solver::new(rules);
                let mut mine = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    mine.push((i, job(&mut sv, i)));
                }
                done.lock().unwrap().extend(mine);
            });
        }
    });
    let mut done = done.into_inner().unwrap();
    done.sort_by_key(|d| d.0);
    done.into_iter().map(|d| d.1).collect()
}

/// Mean and its standard error.
pub fn spread(v: &[f64]) -> (f64, f64) {
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);
    (mean, (var / n).sqrt())
}
