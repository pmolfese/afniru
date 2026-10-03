#!/usr/bin/env bash
# Regenerate tests/fixtures/clust+orig and tests/fixtures/clusterize/*.1D:
# AFNI's own cluster reports (3dClusterize) for the cases tested in
# src/tools/clusterize/mod.rs. Needs AFNI on PATH. Normal `cargo test` never
# runs this.
#
#   tests/fixtures/make_clusterize_fixtures.sh
#
# clust+orig is stat+orig's grid (4x5x6, 2x2x3 mm) holding a made-up
# t-statistic pattern with several clusters of both signs, so that NN1, NN2 and
# NN3 give different answers.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$here"
3dcalc -a stat+orig -datum float -prefix "$work/clust" -overwrite \
  -expr '4*sin(i*2.2+j*1.1)*cos(k*1.9+i*0.7)+1.5*sin(j*3.3+k*0.4)'
3drefit -substatpar 0 fitt 118 -sublabel 0 Tstat "$work/clust+orig"
cp "$work"/clust+orig.* "$here/"
run() { # name, options...
  local name="$1"; shift
  3dClusterize -inset clust+orig -ithr 0 -idat 0 "$@" -pref_map "$work/map" -overwrite \
    2>/dev/null > "clusterize/$name.1D"
}
run nn1_bisided_1.5_min2   -NN 1 -bisided -1.5 1.5 -clust_nvox 2
run nn2_bisided_1.5_min2   -NN 2 -bisided -1.5 1.5 -clust_nvox 2
run nn3_bisided_1.5_min2   -NN 3 -bisided -1.5 1.5 -clust_nvox 2
run nn1_bisided_1.5_min5   -NN 1 -bisided -1.5 1.5 -clust_nvox 5
run nn2_right_2.0_min1     -NN 2 -1sided RIGHT_TAIL 2.0 -clust_nvox 1
run nn2_twosided_1.5_min2  -NN 2 -2sided -1.5 1.5 -clust_nvox 2
# (min size in microliters is checked against this one: 30 µL / 12 µL per voxel = 3 voxels;
# 3dClusterize's own -clust_vol reads its number as VOXELS, so it is not used.)
run nn1_bisided_1.5_min3   -NN 1 -bisided -1.5 1.5 -clust_nvox 3
