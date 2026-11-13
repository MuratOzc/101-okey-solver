use okey_engine::analysis::{Analyzer, Field, Options, Packed, View};
use okey_engine::net::Net;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::sync::Arc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent};

#[derive(Deserialize)]
#[serde(tag = "kind")]
enum Req {
    Start {
        generation: u32,
        view: View,
        seed: u64,
        #[serde(default)]
        options: Options,
        #[serde(default)]
        model: bool,
        field: Field,
    },
    Field {
        generation: u32,
        field: Field,
    },
    Stop,
}

#[derive(Serialize)]
struct Resp {
    generation: u32,
    version: u32,
    deals: Vec<Vec<Packed>>,
    error: Option<String>,
}

struct Job {
    generation: u32,
    an: Analyzer,
    version: u32,
    seed: u64,
    played: u64,
}

thread_local! {
    static JOB: RefCell<Option<Job>> = const { RefCell::new(None) };
    static NET: Option<Arc<Net>> = network(include_bytes!("../model/value.bin"));
    static TEAM_NET: Option<Arc<Net>> = network(include_bytes!("../model/partners.bin"));
}

fn network(bytes: &[u8]) -> Option<Arc<Net>> {
    let mut net = Net::from_bytes(bytes).ok()?;
    net.discards = PLAY_ON_DISCARDS;
    Some(Arc::new(net))
}

const PLAY_ON_DISCARDS: usize = 5;
const TICK_MS: f64 = 50.0;

fn scope() -> DedicatedWorkerGlobalScope {
    js_sys::global().unchecked_into()
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn post(r: &Resp) {
    if let Ok(s) = serde_json::to_string(r) {
        let _ = scope().post_message(&JsValue::from_str(&s));
    }
}

fn schedule(generation: u32) {
    let cb = Closure::once_into_js(move || tick(generation));
    let _ = scope().set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), 0);
}

fn tick(generation: u32) {
    let resp = JOB.with(|j| {
        let mut j = j.borrow_mut();
        let job = j.as_mut().filter(|job| job.generation == generation)?;
        let t0 = now();
        let mut deals = Vec::new();
        while deals.is_empty() || now() - t0 < TICK_MS {
            let seed = job.seed.wrapping_add(job.played.wrapping_mul(0x2545_F491_4F6C_DD1D));
            job.played += 1;
            deals.push(job.an.evaluate(seed));
        }
        Some(Resp {
            generation,
            version: job.version,
            deals,
            error: None,
        })
    });
    if let Some(r) = resp {
        post(&r);
        schedule(generation);
    }
}

fn main() {
    console_error_panic_hook::set_once();
    let onmsg = Closure::<dyn FnMut(MessageEvent)>::new(|e: MessageEvent| {
        let Some(s) = e.data().as_string() else { return };
        match serde_json::from_str::<Req>(&s) {
            Ok(Req::Start {
                generation,
                view,
                seed,
                options,
                model,
                field,
            }) => match Analyzer::with_options(view, seed, options) {
                Ok(mut an) => {
                    for net in [&NET, &TEAM_NET] {
                        if model && let Some(net) = net.with(|net| net.clone()) {
                            an.use_model(net, 0.0, true);
                        }
                    }
                    an.set_field(&field);
                    JOB.with(|j| {
                        *j.borrow_mut() = Some(Job {
                            generation,
                            an,
                            version: field.version,
                            seed,
                            played: 0,
                        })
                    });
                    schedule(generation);
                }
                Err(error) => {
                    JOB.with(|j| *j.borrow_mut() = None);
                    post(&Resp {
                        generation,
                        version: 0,
                        deals: vec![],
                        error: Some(error),
                    });
                }
            },
            Ok(Req::Field { generation, field }) => JOB.with(|j| {
                if let Some(job) = j.borrow_mut().as_mut().filter(|job| job.generation == generation) {
                    job.an.set_field(&field);
                    job.version = field.version;
                }
            }),
            Ok(Req::Stop) => JOB.with(|j| *j.borrow_mut() = None),
            Err(err) => post(&Resp {
                generation: 0,
                version: 0,
                deals: vec![],
                error: Some(err.to_string()),
            }),
        }
    });
    scope().set_onmessage(Some(onmsg.as_ref().unchecked_ref()));
    onmsg.forget();
    let _ = scope().post_message(&JsValue::from_str("ready"));
}
