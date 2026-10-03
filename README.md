# afniru

A native, cross-platform Rust rebuild of the AFNI volume viewer: 2D MRI slices
with AFNI's overlay, threshold and cluster tools, in a modern single-window UI.
Sister project of [sumaru](../sumaru) (the SUMA rebuild). Early development:
see [`afniru_ROADMAP.md`](afniru_ROADMAP.md) for status and
[`afniru_draft.md`](afniru_draft.md) for the design.

## Build and run

Needs Rust 1.99+ and the sibling checkouts `../afni-io` (and `../afni-core`,
via the git-ignored `.cargo/config.toml` patch; remove that file to fetch
afni-core from GitHub instead).

```sh
cargo run --release -- --demo                 # built-in phantom, no data needed
cargo run --release -- anat+orig.HEAD         # AFNI dataset
cargo run --release -- T1.nii.gz func.nii.gz  # NIfTI; first file is the underlay
```

You can also use File ▸ Open… or drop a file on the window.

## Settings

On first run afniru writes `~/.afniru`, a commented file in AFNI's `~/.afnirc`
format with every setting at its default. The theme follows macOS unless
`AFNIRU_THEME` says otherwise.

## Development

```sh
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and
[`docs/GLOSSARY.md`](docs/GLOSSARY.md).
