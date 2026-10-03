#!/bin/bash
# Regenerates the afni_proc.py fixtures in this directory. Needs AFNI on PATH.
#
# The proc.sub-01 scripts are REAL afni_proc.py output (only the AFNI install
# path is replaced by /path/to/abin). The results directories are hand-made:
# tiny 4x5x6 datasets and small text files with the names the script says it
# creates, so afniru's provenance code can be tested without running a
# pipeline. Health-relevant files (out.ss_review.*, warnings) are written by
# hand in the formats AFNI documents; see docs/PROCESSING_RAIL.md.
set -e
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
cd "$work"

# tiny inputs
3dUndump -dimen 4 5 6 -prefix anat -overwrite >/dev/null 2>&1
3dcalc -a anat+orig -expr 'a+x+y+z' -prefix tmp -overwrite >/dev/null 2>&1
idx=$(python3 -c "print(','.join(['0']*30))")
for r in 01 02; do
  3dTcat -prefix epi.r$r "tmp+orig[$idx]" -overwrite >/dev/null 2>&1
  3drefit -TR 2 epi.r$r+orig >/dev/null 2>&1
done
echo "0 10 20" > stim.1D

gen() { # case blocks... -- extra options... -- dsets...
  local name=$1; shift
  mkdir -p "$here/$name"
  afni_proc.py -subj_id sub-01 -copy_anat anat+orig.HEAD -tcat_remove_first_trs 0 \
    -regress_stim_times stim.1D -regress_stim_labels task -regress_basis GAM \
    -regress_motion_per_run -regress_censor_motion 0.3 -regress_censor_outliers 0.05 \
    -regress_apply_mot_types demean deriv -regress_run_clustsim no \
    -script "$here/$name/proc.sub-01" -out_dir sub-01.results "$@" >/dev/null
  # The script comments name the -script path we passed; make them relative.
  sed -i.bak -e "s#$here/$name/##g" -e "s#$(dirname "$(which afni_proc.py)")#/path/to/abin#g" "$here/$name/proc.sub-01"
  rm -f "$here/$name/proc.sub-01.bak"
}

full="-blocks tshift align tlrc volreg blur mask scale regress -align_opts_aea -cost lpc+ZZ -tlrc_base MNI152_2009_template_SSW.nii.gz -tlrc_NL_warp -volreg_align_to MIN_OUTLIER -volreg_align_e2a -volreg_tlrc_warp -blur_size 4.0 -regress_est_blur_epits -regress_est_blur_errts"
gen complete $full -dsets epi.r01+orig.HEAD
gen two_runs $full -dsets epi.r01+orig.HEAD epi.r02+orig.HEAD
gen omitted -blocks tshift volreg regress -volreg_align_to MIN_OUTLIER -dsets epi.r01+orig.HEAD
gen custom_order -blocks tshift blur volreg regress -blur_size 4.0 -dsets epi.r01+orig.HEAD
echo "scripts written to $here"
