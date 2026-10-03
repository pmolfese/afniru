#!/usr/bin/env bash
# Regenerate tests/fixtures/bold+orig (a 4x5x6 grid with 40 time points, TR 2 s)
# and bold_series.txt (3dmaskdump's time series at chosen voxels, "i j k v0 v1 ...").
# Needs AFNI on PATH. Normal `cargo test` never runs this.
#
#   tests/fixtures/make_bold_fixture.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$work"
for t in $(seq 0 39); do
  3dcalc -a "$here/tiny2+orig[0]" -datum float -prefix "b_$t" -overwrite \
    -expr "100+10*sin(i*1.3+j*0.7+k*0.4)+8*cos(($t)*0.45+i*0.9)+0.5*($t)+3*sin(($t)*2.1+j+k)" \
    >/dev/null 2>&1
done
3dTcat -prefix bold -overwrite b_{0..39}+orig.HEAD >/dev/null 2>&1
3drefit -TR 2 bold+orig >/dev/null 2>&1
cp bold+orig.HEAD bold+orig.BRIK.gz "$here/"
: > "$here/bold_series.txt"
for v in "0 0 0" "1 2 3" "3 4 5" "2 0 5" "0 4 1"; do
  set -- $v
  line="$(3dmaskdump -noijk -ibox "$1" "$2" "$3" "$here/bold+orig" 2>/dev/null)"
  echo "$1 $2 $3 $line" >> "$here/bold_series.txt"
done
