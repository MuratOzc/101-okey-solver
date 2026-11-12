use okey_engine::features::{Feat, NREAL, WORDS};
use okey_engine::net::Net;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("net_check <network file>  (reads the .check file the trainer wrote next to it)");
    let net = Net::from_file(&path).expect("network");
    let raw = std::fs::read(format!("{path}.check")).expect("check file");
    let size = WORDS * 8 + NREAL;
    let k = raw.len() / (size + 4);
    let mut worst = 0f32;
    for i in 0..k {
        let rec = &raw[i * size..(i + 1) * size];
        let mut f = Feat {
            bits: [0; WORDS],
            real: [0; NREAL],
        };
        for (w, chunk) in rec[..WORDS * 8].as_chunks::<8>().0.iter().enumerate() {
            f.bits[w] = u64::from_le_bytes(*chunk);
        }
        f.real.copy_from_slice(&rec[WORDS * 8..]);
        let want = f32::from_le_bytes(raw[k * size + 4 * i..k * size + 4 * i + 4].try_into().unwrap());
        worst = worst.max((net.value(&f) - want).abs());
    }
    println!("{k} positions, largest difference {worst:.4} (in units where one point of gap is 3)");
    assert!(worst < 0.05, "this build and the trainer disagree");
}
