//! Synthetic T1-like head phantom and a fake t-map, for development without
//! data on disk (`afniru --demo`) and for tests. Ported from the mockup
//! (`mockups/ui_mockup/src/phantom.rs`). Nothing here is anatomically
//! accurate.
//!
//! Index convention matches AFNI "RAI": i runs R->L, j runs A->P, k runs I->S.
//! Physical coordinates passed to the tissue function are (lr, ap, is) in mm,
//! with +lr = Left, +ap = Anterior, +is = Superior.

use afni_core::stat::{StatKind, StatSpec};

use super::Dataset;

/// Grid size in x.
pub const NX: usize = 150;
/// Grid size in y.
pub const NY: usize = 180;
/// Grid size in z.
pub const NZ: usize = 150;

struct Vol {
    data: Vec<f32>,
}

impl Vol {}

/// The phantom anatomy as a one-sub-brick dataset, 1 mm voxels.
pub fn phantom() -> Dataset {
    Dataset::synthetic("phantom", vec![build_anat().data], vec!["anat".into()])
}

/// A second dataset on the phantom's grid, for tests that switch underlay.
#[cfg(test)]
pub fn tmap_for_tests() -> Dataset {
    tmap()
}

/// A second t-map on the phantom's grid, mirrored left-right, so a stack of
/// two layers shows two different pictures.
#[cfg(test)]
pub fn mirrored_tmap() -> Dataset {
    let mut d = tmap();
    d.name = "tmap_mirror".into();
    if let super::Data::Synthetic(frames) = &mut d.data {
        for f in frames.iter_mut() {
            for k in 0..NZ {
                for j in 0..NY {
                    let row = &mut f[NX * (j + NY * k)..NX * (j + NY * k) + NX];
                    row.reverse();
                }
            }
        }
    }
    d
}

/// A fake t-statistic map on the phantom's grid.
pub fn tmap() -> Dataset {
    let t = build_tstat(&build_anat());
    let mut d = Dataset::synthetic("tmap", vec![t.data], vec!["task#0_Tstat".into()]);
    d.stats = vec![Some(StatSpec::new(StatKind::Ttest, &[118.0], 0.0))];
    d
}

/// voxel index -> (lr, ap, is) mm
fn ijk_to_mm(i: usize, j: usize, k: usize) -> (f32, f32, f32) {
    let lr = -(NX as f32) / 2.0 + i as f32; // i=0 is Right (negative lr)
    let ap = (NY as f32) / 2.0 - j as f32 - 5.0; // j=0 is Anterior
    let is = -(NZ as f32) / 2.0 + k as f32 + 10.0;
    (lr, ap, is)
}

// --- value noise -----------------------------------------------------------

fn hash(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6b343)
        ^ (y as u32).wrapping_mul(0xd8163841)
        ^ (z as u32).wrapping_mul(0xcb1ab31f)
        ^ seed.wrapping_mul(0x9e3779b9);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 32767.5 - 1.0
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn vnoise(x: f32, y: f32, z: f32, seed: u32) -> f32 {
    let (xi, yi, zi) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
    let (xf, yf, zf) = (
        smooth(x - xi as f32),
        smooth(y - yi as f32),
        smooth(z - zi as f32),
    );
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let c = |dx, dy, dz| hash(xi + dx, yi + dy, zi + dz, seed);
    l(
        l(
            l(c(0, 0, 0), c(1, 0, 0), xf),
            l(c(0, 1, 0), c(1, 1, 0), xf),
            yf,
        ),
        l(
            l(c(0, 0, 1), c(1, 0, 1), xf),
            l(c(0, 1, 1), c(1, 1, 1), xf),
            yf,
        ),
        zf,
    )
}

fn fbm(x: f32, y: f32, z: f32, seed: u32, oct: u32) -> f32 {
    let (mut s, mut a, mut f, mut n) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..oct {
        s += a * vnoise(x * f, y * f, z * f, seed + o * 17);
        n += a;
        a *= 0.5;
        f *= 2.03;
    }
    s / n
}

fn ell(p: (f32, f32, f32), c: (f32, f32, f32), r: (f32, f32, f32)) -> f32 {
    let dx = (p.0 - c.0) / r.0;
    let dy = (p.1 - c.1) / r.1;
    let dz = (p.2 - c.2) / r.2;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

const WM: f32 = 0.86;
const GM: f32 = 0.55;
const CSF: f32 = 0.10;

/// Tissue intensity at a physical point.
// The `CSF` default below is overwritten on some paths; kept as in the mockup.
#[allow(unused_assignments)]
fn tissue(p: (f32, f32, f32)) -> f32 {
    let (lr, ap, is) = p;
    // head + neck
    let head = ell(p, (0.0, -2.0, 4.0), (72.0, 94.0, 86.0));
    let neck_r = ((lr / 50.0).powi(2) + ((ap + 12.0) / 56.0).powi(2)).sqrt();
    let in_neck = is < -40.0 && neck_r < 1.0;
    if head > 1.0 && !in_neck {
        return 0.0;
    }
    let mut v;
    if in_neck && head > 0.86 {
        // neck tissue: muscle with a fatty rim
        v = if neck_r > 0.93 {
            0.72
        } else {
            0.42 + 0.05 * fbm(lr / 6.0, ap / 6.0, is / 6.0, 3, 2)
        };
        let spine = ((lr / 9.0).powi(2) + ((ap + 30.0) / 9.0).powi(2)).sqrt();
        if spine < 1.0 {
            v = if spine < 0.55 { 0.35 } else { 0.08 };
        }
        return v;
    }
    // scalp / skull / csf layers
    if head > 0.95 {
        return 0.82 + 0.06 * fbm(lr / 4.0, ap / 4.0, is / 4.0, 5, 2);
    }
    if head > 0.935 {
        return 0.25;
    }
    if head > 0.875 {
        let diploe = (head - 0.905).abs() < 0.012;
        return if diploe { 0.38 } else { 0.05 };
    }
    // eyes + orbital fat
    for sx in [-1.0f32, 1.0] {
        let e = ell(p, (sx * 31.0, 64.0, -18.0), (12.0, 12.0, 12.0));
        if e < 1.0 {
            return if e > 0.86 { 0.45 } else { 0.07 };
        }
        let f = ell(p, (sx * 28.0, 50.0, -18.0), (17.0, 24.0, 15.0));
        if f < 1.0 {
            return 0.88;
        }
    }
    v = CSF; // default inside skull: csf

    // cerebellum (foliated)
    let cb = ell(p, (0.0, -56.0, -28.0), (46.0, 28.0, 19.0));
    if cb < 1.0 && is < -12.0 {
        let n_cb = fbm(lr / 10.0, ap / 10.0, is / 10.0, 13, 2);
        let fol = (((is + 0.45 * ap + 2.0 * n_cb) * 1.1).sin()
            + 0.35 * fbm(lr / 8.0, ap / 8.0, is / 8.0, 11, 2))
        .abs();
        let core = ell(p, (0.0, -50.0, -28.0), (16.0, 14.0, 12.0));
        v = if core < 1.0 || (cb < 0.75 && fol < 0.3) {
            0.8
        } else if fol > 0.93 {
            0.18
        } else {
            0.52
        };
        if lr.abs() < 1.2 && cb > 0.6 {
            v = CSF;
        }
        return v;
    }
    // brainstem
    let bs = ((lr / 12.0).powi(2) + ((ap + 22.0) / 13.0).powi(2)).sqrt();
    if bs < 1.0 && is < -5.0 && is > -60.0 {
        return 0.74;
    }

    // cerebrum: cortex = level sets of (radial depth + folding noise)
    let c = (0.0, -6.0, 14.0);
    let r = ell(p, c, (63.0, 82.0, 62.0));
    let n1 = fbm(lr / 16.0, ap / 16.0, is / 16.0, 1, 2);
    let n2 = fbm(lr / 11.0, ap / 11.0, is / 11.0, 2, 3);
    let surf = 0.97 + 0.025 * n1;
    // temporal lobes hang lower laterally; brainstem occupies the middle
    let base = ap > -40.0 && is < -6.0 - 24.0 * (lr.abs() / 38.0).min(1.0) + 0.12 * ap.max(0.0);
    if r < surf && !base {
        let fissure = lr.abs() < 1.4 + 2.0 * ((is - c.2) / 60.0).max(0.0) && is > -4.0;
        if fissure {
            return CSF;
        }
        // sulci are the zero-set sheets of n2, reaching a variable depth
        let depth = surf - r;
        let dmax = 0.32 + 0.12 * n1;
        let ds = n2.abs();
        v = if depth < dmax && ds < 0.022 {
            0.13
        } else if depth < 0.06 || (depth < dmax + 0.05 && ds < 0.13) {
            GM + 0.04 * n1
        } else {
            WM
        };
        // corpus callosum bridge
        let cc = ell(p, (0.0, -6.0, 18.0), (14.0, 38.0, 4.5));
        if cc < 1.0 {
            v = WM;
        }
        // deep gray: thalamus, putamen, caudate
        for sx in [-1.0f32, 1.0] {
            if ell(p, (sx * 10.0, -16.0, 6.0), (9.0, 14.0, 9.0)) < 1.0 {
                v = 0.66;
            }
            if ell(p, (sx * 25.0, 4.0, 4.0), (5.5, 15.0, 10.0)) < 1.0 {
                v = 0.62;
            }
            if ell(p, (sx * 13.5, 10.0, 16.0), (4.5, 11.0, 7.0)) < 1.0 {
                v = 0.63;
            }
        }
        // lateral ventricles: butterfly shape
        for sx in [-1.0f32, 1.0] {
            let tilt = (is - 18.0) * 0.35 * sx;
            if ell(p, (sx * 8.0 + tilt * 0.3, -4.0, 18.0), (5.0, 30.0, 6.5)) < 1.0 {
                v = CSF;
            }
            if ell(p, (sx * 14.0, -36.0, 14.0), (4.0, 12.0, 5.0)) < 1.0 {
                v = CSF;
            }
        }
        if lr.abs() < 1.6 && ell(p, (0.0, -14.0, 4.0), (2.0, 12.0, 8.0)) < 1.0 {
            v = CSF;
        }
        return v;
    }
    // outside the brain: csf in the cranial cavity, soft tissue below it
    let cav = ell(p, (0.0, -6.0, 12.0), (67.0, 87.0, 68.0));
    let base_z = -6.0 - 24.0 * (lr.abs() / 38.0).min(1.0) + 0.12 * ap.max(0.0);
    if cav < 1.0 && (is > base_z - 2.0 || ap < -40.0) {
        return CSF;
    }
    if cav < 1.05 && is > base_z - 7.0 {
        return 0.06; // skull base
    }
    let fat = fbm(lr / 12.0, ap / 12.0, is / 12.0, 41, 2);
    v = 0.42 + 0.22 * smooth(((fat - 0.05) * 3.0).clamp(0.0, 1.0));
    v
}

fn build_anat() -> Vol {
    let mut data = vec![0.0f32; NX * NY * NZ];
    for k in 0..NZ {
        for j in 0..NY {
            for i in 0..NX {
                let p = ijk_to_mm(i, j, k);
                data[i + NX * (j + NY * k)] = tissue(p);
            }
        }
    }
    // soften edges: separable 3-tap blur
    for axis in 0..3 {
        let stride = [1, NX, NX * NY][axis];
        let n = [NX, NY, NZ][axis];
        let src = data.clone();
        for idx in 0..data.len() {
            let pos = (idx / stride) % n;
            if pos == 0 || pos == n - 1 {
                continue;
            }
            data[idx] = 0.25 * src[idx - stride] + 0.5 * src[idx] + 0.25 * src[idx + stride];
        }
    }
    // bias field + noise
    for k in 0..NZ {
        for j in 0..NY {
            for i in 0..NX {
                let idx = i + NX * (j + NY * k);
                let (lr, ap, is) = ijk_to_mm(i, j, k);
                let bias = 1.0 + 0.06 * fbm(lr / 60.0, ap / 60.0, is / 60.0, 9, 1);
                let noise = 0.025 * hash(i as i32, j as i32, k as i32, 77);
                data[idx] = if data[idx] < 0.015 {
                    0.0
                } else {
                    (data[idx] * bias + noise).max(0.0)
                };
            }
        }
    }
    Vol { data }
}

/// Fake t-statistic: a few blobs + smooth noise, masked to brain.
fn build_tstat(anat: &Vol) -> Vol {
    let blobs: [((f32, f32, f32), f32, f32); 7] = [
        ((-17.0, -72.0, 8.0), 8.0, 11.0), // right visual
        ((18.0, -74.0, 6.0), 7.2, 11.0),  // left visual
        ((-52.0, -16.0, 8.0), 5.8, 8.0),  // right auditory
        ((53.0, -18.0, 6.0), 6.3, 8.0),   // left auditory
        ((0.0, 48.0, 10.0), -5.2, 11.0),  // mPFC (deactivation)
        ((0.0, -50.0, 30.0), -4.6, 10.0), // PCC (deactivation)
        ((38.0, -22.0, 52.0), 6.0, 8.0),  // left motor
    ];
    let mut data = vec![0.0f32; NX * NY * NZ];
    for k in 0..NZ {
        for j in 0..NY {
            for i in 0..NX {
                let idx = i + NX * (j + NY * k);
                let a = anat.data[idx];
                if !(0.4..0.95).contains(&a) {
                    continue;
                }
                let p = ijk_to_mm(i, j, k);
                // brain only (rough)
                if ell(p, (0.0, -6.0, 14.0), (63.0, 82.0, 62.0)) > 1.0 {
                    continue;
                }
                let mut t = 1.6 * fbm(p.0 / 9.0, p.1 / 9.0, p.2 / 9.0, 31, 3);
                for (c, amp, sig) in blobs {
                    let d2 = (p.0 - c.0).powi(2) + (p.1 - c.1).powi(2) + (p.2 - c.2).powi(2);
                    t += amp * (-d2 / (2.0 * sig * sig)).exp();
                }
                // gray matter weighted, like real BOLD
                let gm_w = if a < 0.75 { 1.0 } else { 0.7 };
                data[idx] = t * gm_w;
            }
        }
    }
    Vol { data }
}
