use crate::features::{Feat, LAYOUT, NBITS, NBITS_PLAIN, NBITS_V1, NREAL, NREAL_PLAIN};

const MAGIC: &[u8; 4] = b"OKNN";
const MAX_WIDTH: usize = 1024;

// Weights are stored by input, not by output as in the file.
struct Layer {
    outputs: usize,
    weights: Vec<f32>,
    bias: Vec<f32>,
}

/// One position's first-layer sums, for valuing positions that differ from it in a few inputs.
pub struct Near {
    feat: Feat,
    sums: [f32; MAX_WIDTH],
}

pub struct Net {
    nbits: usize,
    nreal: usize,
    first: Layer,
    rest: Vec<Layer>,
    pub scale: f32,
    pub discards: usize,
    pub partners: bool,
}

fn word(bytes: &[u8], at: &mut usize) -> Result<u32, String> {
    let b = bytes.get(*at..*at + 4).ok_or("network file is cut short")?;
    *at += 4;
    Ok(u32::from_le_bytes(b.try_into().unwrap()))
}

fn floats(bytes: &[u8], at: &mut usize, n: usize) -> Result<Vec<f32>, String> {
    let b = bytes.get(*at..*at + 4 * n).ok_or("network file is cut short")?;
    *at += 4 * n;
    Ok(b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
}

impl Net {
    // Must match `export` in train/train.py.
    pub fn from_bytes(bytes: &[u8]) -> Result<Net, String> {
        if bytes.get(..4) != Some(MAGIC) {
            return Err("not a network file".into());
        }
        let mut at = 4;
        let (layout, nbits, nreal) = (word(bytes, &mut at)?, word(bytes, &mut at)?, word(bytes, &mut at)?);
        let (nbits, nreal) = (nbits as usize, nreal as usize);
        if !matches!(
            (layout, nbits, nreal),
            (1, NBITS_V1, NREAL_PLAIN) | (LAYOUT, NBITS_PLAIN, NREAL_PLAIN) | (LAYOUT, NBITS, NREAL)
        ) {
            return Err(format!(
                "network was trained on a feature layout this build does not know ({layout}: {nbits}+{nreal} inputs)"
            ));
        }
        let partners = layout == LAYOUT && word(bytes, &mut at)? != 0;
        let scale = f32::from_bits(word(bytes, &mut at)?);
        let count = word(bytes, &mut at)? as usize;
        let mut layers = Vec::new();
        let mut width = nbits + nreal;
        for _ in 0..count {
            let (inputs, outputs) = (word(bytes, &mut at)? as usize, word(bytes, &mut at)? as usize);
            if inputs != width || outputs == 0 || outputs > MAX_WIDTH {
                return Err("network layers do not fit together".into());
            }
            let by_output = floats(bytes, &mut at, inputs * outputs)?;
            let mut weights = vec![0f32; by_output.len()];
            for o in 0..outputs {
                for i in 0..inputs {
                    weights[i * outputs + o] = by_output[o * inputs + i];
                }
            }
            let bias = floats(bytes, &mut at, outputs)?;
            layers.push(Layer { outputs, weights, bias });
            width = outputs;
        }
        if width != 1 || layers.len() < 2 {
            return Err("network must end in a single output".into());
        }
        let first = layers.remove(0);
        Ok(Net {
            nbits,
            nreal,
            first,
            rest: layers,
            scale,
            discards: crate::tile::NIDS,
            partners,
        })
    }

    pub fn from_file(path: &str) -> Result<Net, String> {
        Net::from_bytes(&std::fs::read(path).map_err(|e| format!("{path}: {e}"))?)
    }

    pub fn full(&self) -> bool {
        self.nbits == NBITS
    }

    pub fn value(&self, f: &Feat) -> f32 {
        let mut a = [0f32; MAX_WIDTH];
        self.first_sums(f, &mut a);
        self.rest_value(&mut a)
    }

    pub fn near(&self, f: &Feat) -> Near {
        let mut near = Near {
            feat: *f,
            sums: [0f32; MAX_WIDTH],
        };
        self.first_sums(f, &mut near.sums);
        near
    }

    /// The same as `value(f)` up to rounding, at the cost of the inputs in which `f` differs from `near`.
    pub fn value_near(&self, near: &Near, f: &Feat) -> f32 {
        let h = self.first.outputs;
        let mut a = [0f32; MAX_WIDTH];
        a[..h].copy_from_slice(&near.sums[..h]);
        for (w, (&now, &was)) in f.bits.iter().zip(&near.feat.bits).enumerate() {
            let mut changed = now ^ was;
            while changed != 0 {
                let bit = changed.trailing_zeros();
                let i = w * 64 + bit as usize;
                changed &= changed - 1;
                if i >= self.nbits {
                    break;
                }
                let col = &self.first.weights[i * h..(i + 1) * h];
                if now >> bit & 1 == 1 {
                    for (x, &c) in a[..h].iter_mut().zip(col) {
                        *x += c;
                    }
                } else {
                    for (x, &c) in a[..h].iter_mut().zip(col) {
                        *x -= c;
                    }
                }
            }
        }
        for (j, (&now, &was)) in f.real.iter().zip(&near.feat.real).enumerate().take(self.nreal) {
            if now != was {
                let x = (now as f32 - was as f32) / 255.0;
                let i = self.nbits + j;
                let col = &self.first.weights[i * h..(i + 1) * h];
                for (y, &c) in a[..h].iter_mut().zip(col) {
                    *y += x * c;
                }
            }
        }
        self.rest_value(&mut a)
    }

    /// How many inputs differ; a position far from `near` is cheaper to value from scratch.
    pub fn distance(near: &Near, f: &Feat) -> u32 {
        f.bits.iter().zip(&near.feat.bits).map(|(a, b)| (a ^ b).count_ones()).sum()
    }

    fn first_sums(&self, f: &Feat, a: &mut [f32; MAX_WIDTH]) {
        let h = self.first.outputs;
        a[..h].copy_from_slice(&self.first.bias);
        for (w, &bits) in f.bits.iter().enumerate() {
            let mut bits = bits;
            while bits != 0 {
                let i = w * 64 + bits.trailing_zeros() as usize;
                bits &= bits - 1;
                if i >= self.nbits {
                    break;
                }
                let col = &self.first.weights[i * h..(i + 1) * h];
                for (x, &c) in a[..h].iter_mut().zip(col) {
                    *x += c;
                }
            }
        }
        for (j, &v) in f.real.iter().enumerate().take(self.nreal) {
            if v != 0 {
                let x = v as f32 / 255.0;
                let i = self.nbits + j;
                let col = &self.first.weights[i * h..(i + 1) * h];
                for (y, &c) in a[..h].iter_mut().zip(col) {
                    *y += x * c;
                }
            }
        }
    }

    fn rest_value(&self, a: &mut [f32; MAX_WIDTH]) -> f32 {
        let mut b = [0f32; MAX_WIDTH];
        let mut width = self.first.outputs;
        for layer in &self.rest {
            let n = layer.outputs;
            b[..n].copy_from_slice(&layer.bias);
            for (i, &x) in a[..width].iter().enumerate() {
                if x > 0.0 {
                    let col = &layer.weights[i * n..(i + 1) * n];
                    for (y, &c) in b[..n].iter_mut().zip(col) {
                        *y += x * c;
                    }
                }
            }
            width = n;
            a[..width].copy_from_slice(&b[..width]);
        }
        a[0] * self.scale
    }
}
