use crate::state::{Stage, State};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::RefCell;

const HAND_KEY: &str = "okey101.hand.v1";
const SAVED_KEY: &str = "okey101.replays.v1";
const UNDO_KEY: &str = "okey101.undo.v1";
/// Screen-only fields, left out of the record.
const SKIP: [&str; 2] = ["buffer", "mode"];

/// What the app was showing when an entry was made.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Shown {
    /// The leading candidates as "label: mean" (score gap, lower is better), with the deals behind them.
    pub advice: Vec<String>,
    pub deals: u64,
    /// The estimate line under each opponent: right, across, left.
    pub reads: Vec<String>,
}

/// One entry: every field that changed, as a path and its new value.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Event {
    pub at: f64,
    pub changed: Vec<(String, Value)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown: Option<Shown>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HandLog {
    pub start: Value,
    pub events: Vec<Event>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Saved {
    pub saved_at: String,
    pub hand_no: u8,
    /// False when a new hand or match was started before this one was scored.
    pub complete: bool,
    pub scores: Option<[i32; 4]>,
    pub start: Value,
    pub events: Vec<Event>,
    pub end: Value,
}

thread_local! {
    static LOG: RefCell<Option<HandLog>> = RefCell::new(load(HAND_KEY));
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn load<T: for<'de> Deserialize<'de>>(key: &str) -> Option<T> {
    storage()?.get_item(key).ok()?.and_then(|j| serde_json::from_str(&j).ok())
}

/// Err when the browser refused to store it (usually: storage full).
fn store<T: Serialize>(key: &str, v: &T) -> Result<(), ()> {
    let st = storage().ok_or(())?;
    let j = serde_json::to_string(v).map_err(|_| ())?;
    st.set_item(key, &j).map_err(|_| ())
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn snapshot(s: &State) -> Value {
    let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
    if let Value::Object(m) = &mut v {
        for k in SKIP {
            m.remove(k);
        }
    }
    v
}

fn diff(a: &Value, b: &Value, path: &str, out: &mut Vec<(String, Value)>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, vb) in y {
                let va = x.get(k).unwrap_or(&Value::Null);
                diff(va, vb, &format!("{path}/{k}"), out);
            }
            for k in x.keys().filter(|k| !y.contains_key(*k)) {
                out.push((format!("{path}/{k}"), Value::Null));
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() && x.iter().any(Value::is_object) => {
            for (i, (va, vb)) in x.iter().zip(y).enumerate() {
                diff(va, vb, &format!("{path}/{i}"), out);
            }
        }
        _ if a != b => out.push((path.to_string(), b.clone())),
        _ => {}
    }
}

fn save_log(log: &Option<HandLog>) {
    match log {
        Some(l) => {
            let _ = store(HAND_KEY, l);
        }
        None => {
            if let Some(st) = storage() {
                let _ = st.remove_item(HAND_KEY);
            }
        }
    }
}

pub fn saved() -> Vec<Saved> {
    load(SAVED_KEY).unwrap_or_default()
}

/// Record an entry. Err with a message when a finished hand could not be stored.
pub fn on_act(before: &State, after: &State, shown: Option<Shown>) -> Result<(), String> {
    let mut result = Ok(());
    LOG.with(|cell| {
        let mut log = cell.borrow_mut();
        if after.stage == Stage::Setup && before.stage != Stage::Setup {
            if let Some(l) = log.take() {
                let mut all = saved();
                all.push(Saved {
                    saved_at: js_sys::Date::new_0().to_iso_string().into(),
                    hand_no: before.hand_no,
                    complete: before.stage == Stage::Over,
                    scores: before.hand_scores(),
                    start: l.start,
                    events: l.events,
                    end: snapshot(before),
                });
                if store(SAVED_KEY, &all).is_err() {
                    result = Err("El kaydı telefona yazılamadı: yer dolmuş olabilir. Ayarlardan kayıtları indirip silin.".to_string());
                }
            }
        } else if after.stage != Stage::Setup {
            let l = log.get_or_insert_with(|| HandLog {
                start: snapshot(if before.stage == Stage::Setup { after } else { before }),
                events: vec![],
            });
            if before.stage != Stage::Setup {
                let mut changed = Vec::new();
                diff(&snapshot(before), &snapshot(after), "", &mut changed);
                l.events.push(Event { at: now(), changed, shown });
            }
        }
        save_log(&log);
    });
    result
}

/// Undo the record of the entry that turned `prev` into `cur`.
pub fn on_undo(cur: &State, prev: &State) {
    LOG.with(|cell| {
        let mut log = cell.borrow_mut();
        if cur.stage == Stage::Setup && prev.stage != Stage::Setup {
            let mut all = saved();
            if let Some(last) = all.pop() {
                *log = Some(HandLog {
                    start: last.start,
                    events: last.events,
                });
                let _ = store(SAVED_KEY, &all);
            }
        } else if prev.stage == Stage::Setup {
            *log = None;
        } else if let Some(l) = log.as_mut() {
            l.events.pop();
        }
        save_log(&log);
    });
}

pub fn export() -> String {
    serde_json::to_string(&saved()).unwrap_or_default()
}

pub fn clear() {
    if let Some(st) = storage() {
        let _ = st.remove_item(SAVED_KEY);
    }
}

pub fn load_undo() -> Vec<State> {
    load(UNDO_KEY).unwrap_or_default()
}

/// Keeps as much of the newest history as the browser will store.
pub fn save_undo(u: &[State]) {
    let mut from = 0;
    while store(UNDO_KEY, &&u[from..]).is_err() && from < u.len() {
        from += (u.len() - from).div_ceil(2);
    }
}
