# Glossary

| Term | Meaning |
|---|---|
| Dataset | One volume file (AFNI `.HEAD`/`.BRIK` or NIfTI) with one or more sub-bricks. |
| Sub-brick | One 3D volume within a dataset: a time point, a beta, a statistic. AFNI's term. |
| Underlay (ULay) | The anatomical image shown in gray. |
| Overlay (OLay) | The functional map drawn in color over the underlay. |
| RAI / RAS | Axis orders. AFNI's native order is RAI (DICOM-style); `afni-io` gives both. |
| Radiological | Display with the subject's right on the screen's left (AFNI's default). Neurological is the reverse (`AFNI_LEFT_IS_LEFT = YES`). |
| TR | Repetition time: seconds between time points in a time series. |
| `~/.afniru` | Preferences file, in `~/.afnirc` format. |
