use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    println!("cargo:rustc-env=BUILD_ID={secs}");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../engine/src");
    println!("cargo:rerun-if-changed=model");
}
