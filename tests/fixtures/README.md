# Fixtures

* `tiny2+orig`, `obl+orig`, `*_maskdump*.txt`: see the tests in
  `src/data/load.rs` and `src/render/slice.rs`.
* `stat+orig`: made from `tiny2+orig'[0]'` with
  `3dcalc -expr 'a/60'`, then `3drefit -substatpar 0 fitt 118 -sublabel 0 Tstat`
  and `3drefit -addFDR`. One t-statistic sub-brick (118 df), values -5.45..3.60.
  Golden values from AFNI (used by `src/tools/overlay`):
  * `cdf -t2p fitt 3.1 118` -> p = 0.00242029
  * `cdf -t2p fitt 2.0 118` -> p = 0.0477969
  * `cdf -p2t fitt 0.01 118` -> t = 2.61814
  * `fdrval stat+orig 0 3.1` -> q = 0.0057026 (see the test for the others)
* `clust+orig`, `clusterize/*.1D`: a made-up t-statistic pattern on `stat+orig`'s grid with
  clusters of both signs, and AFNI's own `3dClusterize` reports for several options (NN1/2/3,
  bisided, one- and two-sided, minimum sizes). Regenerate both with
  `make_clusterize_fixtures.sh`. Used by `src/tools/clusterize/compute.rs` and `src/app.rs`.
* `processing/`: see `docs/PROCESSING_RAIL.md`.
