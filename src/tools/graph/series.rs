//! The arithmetic behind the Graph view, with no interface: which voxels a
//! matrix of graphs shows, what is plotted from a stored series (ignore,
//! percent of the mean, detrend), its summary numbers, and stimulus blocks.

use afni_core::signal::Detrend as CoreDetrend;

use crate::geom::{GridOrient, Plane};
use crate::render::slice::PlaneMap;
use crate::session::series::{Detrend, SeriesSettings};

/// The voxels of an `n × n` matrix centered on `center`, as rows from the top
/// of the screen to the bottom and columns from left to right in the display
/// of `plane` (so the matrix looks like the slice). `None` where the matrix
/// leaves the volume.
pub fn matrix_voxels(
    dims: [usize; 3],
    orient: &GridOrient,
    plane: Plane,
    left_is_left: bool,
    center: [usize; 3],
    n: u8,
) -> Vec<Vec<Option<[usize; 3]>>> {
    let map = PlaneMap::new(dims, orient, plane, left_is_left);
    let (col, row) = map.pixel(center);
    let index = center[map.fixed_axis];
    let half = i64::from(n / 2);
    (-half..=half)
        .map(|dr| {
            (-half..=half)
                .map(|dc| {
                    let c = col as i64 + dc;
                    let r = row as i64 + dr;
                    (c >= 0 && r >= 0 && (c as usize) < map.width && (r as usize) < map.height)
                        .then(|| map.voxel(c as usize, r as usize, index))
                })
                .collect()
        })
        .collect()
}

/// What is plotted for a stored series, without a fit (see [`plotted_with_fit`]).
#[cfg(test)]
pub fn plotted(raw: &[f32], settings: &SeriesSettings) -> (usize, Vec<f64>) {
    let (first, data, _) = plotted_with_fit(raw, None, settings);
    (first, data)
}

/// Like [`plotted`], and the fit drawn on the same scale: percent of the
/// *data's* mean and minus the trend that was removed from the data, so the
/// fit stays on top of the points it fits. `None` when the fit's length
/// differs from the data's.
pub fn plotted_with_fit(
    raw: &[f32],
    fit: Option<&[f32]>,
    settings: &SeriesSettings,
) -> (usize, Vec<f64>, Option<Vec<f64>>) {
    let first = settings.ignore.min(raw.len());
    let data: Vec<f64> = raw[first..].iter().map(|&v| f64::from(v)).collect();
    let mean = if data.is_empty() {
        0.0
    } else {
        data.iter().sum::<f64>() / data.len() as f64
    };
    let scale = |v: f64| {
        if settings.percent && mean != 0.0 {
            (v / mean - 1.0) * 100.0
        } else {
            v
        }
    };
    let scaled: Vec<f64> = data.iter().map(|&v| scale(v)).collect();
    let mut detrended = scaled.clone();
    match settings.detrend {
        Detrend::None => CoreDetrend::None,
        Detrend::Mean => CoreDetrend::Mean,
        Detrend::Linear => CoreDetrend::Linear,
        Detrend::Quadratic => CoreDetrend::Quadratic,
    }
    .apply(&mut detrended);
    let fit_out = fit.filter(|f| f.len() == raw.len()).map(|f| {
        f[first..]
            .iter()
            .enumerate()
            .map(|(i, &v)| scale(f64::from(v)) - (scaled[i] - detrended[i]))
            .collect()
    });
    (first, detrended, fit_out)
}

/// Summary numbers of a series.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    /// Mean.
    pub mean: f64,
    /// Sample standard deviation (0 for fewer than two points).
    pub sd: f64,
    /// Smallest value.
    pub min: f64,
    /// Largest value.
    pub max: f64,
}

/// Mean, standard deviation and range of the finite values (`None` if there
/// are none).
pub fn stats(values: &[f64]) -> Option<Stats> {
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    let n = finite.len();
    if n == 0 {
        return None;
    }
    let mean = finite.iter().sum::<f64>() / n as f64;
    let sd = if n > 1 {
        (finite.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt()
    } else {
        0.0
    };
    Some(Stats {
        mean,
        sd,
        min: finite.iter().copied().fold(f64::INFINITY, f64::min),
        max: finite.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    })
}

/// The fraction of the variance of `data` that `fit` explains: `1 - SSE/SST`.
/// `None` when the series differ in length, are shorter than 2, or `data` is
/// constant.
pub fn r_squared(data: &[f64], fit: &[f64]) -> Option<f64> {
    if data.len() != fit.len() || data.len() < 2 {
        return None;
    }
    let mean = data.iter().sum::<f64>() / data.len() as f64;
    let sst: f64 = data.iter().map(|v| (v - mean).powi(2)).sum();
    if sst == 0.0 {
        return None;
    }
    let sse: f64 = data.iter().zip(fit).map(|(d, f)| (d - f).powi(2)).sum();
    Some(1.0 - sse / sst)
}

/// On/off from a regressor: on where the value is more than half the largest
/// absolute value.
pub fn stim_from_column(column: &[f64]) -> Vec<bool> {
    let top = column.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    column.iter().map(|v| top > 0.0 && *v > 0.5 * top).collect()
}

/// Read a `.1D` file's first column as a stimulus.
pub fn load_stim(path: &std::path::Path) -> Result<crate::session::series::Stim, String> {
    let oned = afni_io::onedee::OneD::read(path).map_err(|e| e.to_string())?;
    let column = oned
        .column(0)
        .filter(|c| !c.is_empty())
        .ok_or_else(|| format!("{} has no numbers", path.display()))?;
    Ok(crate::session::series::Stim {
        name: path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into(),
        ),
        on: stim_from_column(&column),
    })
}

/// The runs of "on" as `[start, end)` pairs of time points.
pub fn stim_blocks(on: &[bool]) -> Vec<(usize, usize)> {
    let mut blocks = Vec::new();
    let mut start = None;
    for (t, &v) in on.iter().enumerate() {
        match (v, start) {
            (true, None) => start = Some(t),
            (false, Some(s)) => {
                blocks.push((s, t));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        blocks.push((s, on.len()));
    }
    blocks
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::data::load::load;

    fn bold() -> crate::data::Dataset {
        load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bold+orig"),
            0,
        )
        .unwrap()
    }

    fn settings() -> SeriesSettings {
        SeriesSettings::default()
    }

    // ---- extraction against 3dmaskdump (tests/fixtures/bold_series.txt) ----

    #[test]
    fn the_series_at_a_voxel_matches_3dmaskdump() {
        let ds = bold();
        assert_eq!((ds.nvols, ds.tr), (40, Some(2.0)));
        let text = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bold_series.txt"),
        )
        .unwrap();
        let mut checked = 0;
        for line in text.lines() {
            let v: Vec<f64> = line
                .split_whitespace()
                .map(|x| x.parse().unwrap())
                .collect();
            let ijk = [v[0] as usize, v[1] as usize, v[2] as usize];
            let ours = ds.series(ijk).unwrap();
            assert_eq!(ours.len(), 40);
            for (t, (o, theirs)) in ours.iter().zip(&v[3..]).enumerate() {
                // 3dmaskdump prints 6 significant digits.
                assert!(
                    (f64::from(*o) - theirs).abs() <= 1e-5 * theirs.abs().max(1.0),
                    "voxel {ijk:?}, t={t}: {o} vs {theirs}"
                );
            }
            checked += 1;
        }
        assert_eq!(checked, 5);
    }

    #[test]
    fn a_voxel_outside_the_grid_has_no_series() {
        let ds = bold();
        assert!(ds.series([4, 0, 0]).is_none());
        assert!(ds.series([0, 5, 0]).is_none());
        assert!(ds.series([0, 0, 6]).is_none());
    }

    #[test]
    fn a_synthetic_series_reads_the_same_voxel_in_every_frame() {
        let (b, fit) = crate::data::synthetic::bold();
        let s = b.series([15, 18, 15]).unwrap();
        assert_eq!(s.len(), crate::data::synthetic::BOLD_TRS);
        let f = fit.series([15, 18, 15]).unwrap();
        // The fit is the data without the noise: close, not equal.
        assert!(s.iter().zip(&f).all(|(a, b)| (a - b).abs() <= 12.0 + 1e-3));
        assert!(s.iter().zip(&f).any(|(a, b)| a != b));
    }

    // ---- what is plotted ----

    #[test]
    fn ignore_skips_the_first_points() {
        let raw = [9.0, 8.0, 1.0, 2.0, 3.0];
        let (first, v) = plotted(
            &raw,
            &SeriesSettings {
                ignore: 2,
                ..settings()
            },
        );
        assert_eq!((first, v), (2, vec![1.0, 2.0, 3.0]));
        let (first, v) = plotted(
            &raw,
            &SeriesSettings {
                ignore: 99,
                ..settings()
            },
        );
        assert_eq!((first, v.len()), (5, 0)); // clamped
    }

    #[test]
    fn percent_is_relative_to_the_mean_of_what_is_plotted() {
        let (_, v) = plotted(
            &[1000.0, 100.0, 120.0, 80.0],
            &SeriesSettings {
                ignore: 1,
                percent: true,
                ..settings()
            },
        );
        for (got, want) in v.iter().zip([0.0, 20.0, -20.0]) {
            assert!((got - want).abs() < 1e-9, "{v:?}");
        }
    }

    #[test]
    fn detrending_removes_a_line_and_matches_afnis_arithmetic() {
        let raw: Vec<f32> = (0..20).map(|t| 5.0 + 2.0 * t as f32).collect();
        let (_, lin) = plotted(
            &raw,
            &SeriesSettings {
                detrend: Detrend::Linear,
                ..settings()
            },
        );
        assert!(lin.iter().all(|v| v.abs() < 1e-4), "{lin:?}");
        let (_, mean) = plotted(
            &raw,
            &SeriesSettings {
                detrend: Detrend::Mean,
                ..settings()
            },
        );
        assert!(mean.iter().sum::<f64>().abs() < 1e-4);
        assert!((mean[0] + 19.0).abs() < 1e-4);
        let (_, none) = plotted(&raw, &settings());
        assert_eq!(none[3], 11.0);
    }

    #[test]
    fn the_fit_follows_the_same_scaling_and_trend_as_the_data() {
        // Data = a line plus a block; fit = the same line plus the same block.
        let line = |t: usize| 100.0 + 2.0 * t as f32;
        let block = |t: usize| if (5..10).contains(&t) { 10.0 } else { 0.0 };
        let data: Vec<f32> = (0..20).map(|t| line(t) + block(t)).collect();
        let fit = data.clone();
        for detrend in Detrend::ALL {
            for percent in [false, true] {
                let s = SeriesSettings {
                    detrend,
                    percent,
                    ignore: 2,
                    ..settings()
                };
                let (_, d, f) = plotted_with_fit(&data, Some(&fit), &s);
                let f = f.unwrap();
                assert!(
                    d.iter().zip(&f).all(|(a, b)| (a - b).abs() < 1e-9),
                    "{detrend:?} {percent}"
                );
            }
        }
        // A fit of another length is not drawn.
        assert!(
            plotted_with_fit(&data, Some(&fit[..5]), &settings())
                .2
                .is_none()
        );
        assert!(plotted_with_fit(&data, None, &settings()).2.is_none());
    }

    #[test]
    fn stats_and_r_squared() {
        let s = stats(&[1.0, 2.0, 3.0, 4.0, f64::NAN]).unwrap();
        assert_eq!((s.mean, s.min, s.max), (2.5, 1.0, 4.0));
        assert!((s.sd - 1.2909944).abs() < 1e-6);
        assert!(stats(&[]).is_none());
        assert_eq!(stats(&[7.0]).unwrap().sd, 0.0);
        assert_eq!(r_squared(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]), Some(1.0));
        assert!((r_squared(&[1.0, 2.0, 3.0], &[2.0, 2.0, 2.0]).unwrap()).abs() < 1e-12);
        assert_eq!(r_squared(&[1.0, 1.0], &[1.0, 1.0]), None); // constant data
        assert_eq!(r_squared(&[1.0, 2.0], &[1.0]), None);
    }

    #[test]
    fn stimulus_blocks_are_runs_of_on() {
        let on = stim_from_column(&[0.0, 1.0, 1.0, 0.0, 0.2, 1.0]);
        assert_eq!(on, [false, true, true, false, false, true]);
        assert_eq!(stim_blocks(&on), [(1, 3), (5, 6)]);
        assert!(stim_blocks(&[false, false]).is_empty());
        assert_eq!(stim_blocks(&[true, true]), [(0, 2)]);
        assert_eq!(stim_from_column(&[0.0, 0.0]), [false, false]);
    }

    #[test]
    fn a_1d_file_becomes_a_stimulus() {
        let dir = crate::testutil::TempDir::new("stim");
        let path = dir.path().join("block.1D");
        std::fs::write(&path, "# a comment\n0\n0\n1\n1\n0\n1\n").unwrap();
        let stim = load_stim(&path).unwrap();
        assert_eq!(stim.name, "block.1D");
        assert_eq!(stim.on, [false, false, true, true, false, true]);
        assert!(load_stim(&dir.path().join("missing.1D")).is_err());
        let empty = dir.path().join("empty.1D");
        std::fs::write(&empty, "# nothing\n").unwrap();
        assert!(load_stim(&empty).is_err());
    }

    // ---- the matrix ----

    #[test]
    fn a_matrix_is_laid_out_like_the_slice_and_clipped_at_the_edge() {
        let ds = bold(); // 4x5x6, RAI
        let around = |c, n| matrix_voxels(ds.dims, &ds.orient, Plane::Axial, false, c, n);
        let one = around([2, 2, 3], 1);
        assert_eq!(one, [[Some([2, 2, 3])]]);
        let three = around([2, 2, 3], 3);
        assert_eq!(three.len(), 3);
        assert!(three.iter().all(|r| r.len() == 3));
        assert_eq!(three[1][1], Some([2, 2, 3])); // the center
        // Axial: the neighbors are in the same slice (k fixed).
        assert!(three.iter().flatten().flatten().all(|v| v[2] == 3));
        // Radiological display: screen-right is the subject's left, which is
        // +i for RAI data, so the column to the right has larger i.
        assert_eq!(three[1][2], Some([3, 2, 3]));
        // Rows go from anterior (top, small j) to posterior.
        assert_eq!(three[0][1], Some([2, 1, 3]));
        // At the corner of the slice, part of the matrix is outside.
        let corner = around([0, 0, 0], 3);
        assert_eq!(corner.iter().flatten().filter(|v| v.is_none()).count(), 5);
    }
}
