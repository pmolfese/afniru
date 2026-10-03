//! Loading datasets and listing folders off the interface thread.
//!
//! Opening a dataset can take a long time (big 4D files, a network drive), so
//! each load runs on its own worker thread while the window stays usable. The
//! file reader cannot report progress, so what is shown is the dataset's name
//! and size and the time spent so far.
//!
//! Loads run in parallel but are *applied in the order they were asked for*:
//! `afniru anat func1 func2` makes `anat` the underlay and the others layers
//! in that order, however fast each one reads.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crate::data::{Dataset, load};
use crate::session::action::LoadRole;

/// A folder's datasets, as listed for the user to pick from.
#[derive(Debug, Clone, PartialEq)]
pub struct FolderListing {
    /// The folder.
    pub dir: PathBuf,
    /// Its datasets, sorted by name; `None` while it is being read.
    pub entries: Option<Vec<FolderEntry>>,
    /// Why it could not be read.
    pub error: Option<String>,
}

/// One dataset in a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderEntry {
    /// The name shown (`anat+tlrc`, `func.nii.gz`).
    pub label: String,
    /// What to open: for an AFNI dataset the `prefix+view` path, which `afni-io`
    /// resolves to the `.HEAD`/`.BRIK` pair.
    pub path: PathBuf,
}

/// What the interface shows about a load in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadingInfo {
    /// Which load (for cancelling).
    pub id: u64,
    /// The dataset's name.
    pub name: String,
    /// Its size on disk, when known.
    pub bytes: Option<u64>,
    /// Time spent so far.
    pub elapsed: Duration,
    /// What it will become.
    pub role: LoadRole,
    /// Finished reading but waiting for the loads asked for before it.
    pub waiting: bool,
}

/// A folder's listing that arrived: the folder, and its datasets or the error.
pub type Listed = (PathBuf, Result<Vec<FolderEntry>, String>);

/// A finished load, ready to be applied.
#[derive(Debug)]
pub struct Loaded {
    /// What it was asked to become.
    pub role: LoadRole,
    /// The dataset, or why it could not be read.
    pub result: Result<Dataset, String>,
    /// The path asked for.
    pub path: PathBuf,
}

struct Job {
    id: u64,
    path: PathBuf,
    role: LoadRole,
    bytes: Option<u64>,
    started: Instant,
    state: State,
}

enum State {
    Running(Receiver<Result<Dataset, String>>),
    Done(Box<Result<Dataset, String>>),
}

struct Scan {
    dir: PathBuf,
    rx: Receiver<Result<Vec<FolderEntry>, String>>,
}

/// The loads and folder scans in flight.
pub struct Loader {
    next_id: u64,
    jobs: Vec<Job>,
    scans: Vec<Scan>,
    /// Run on worker threads (the app) or on the caller's (tests).
    background: bool,
}

impl Default for Loader {
    /// A threaded loader; in tests a synchronous one.
    fn default() -> Self {
        Self::new(!cfg!(test))
    }
}

impl Loader {
    /// A loader working on worker threads (`true`) or on the caller's.
    pub fn new(background: bool) -> Self {
        Self {
            next_id: 0,
            jobs: Vec::new(),
            scans: Vec::new(),
            background,
        }
    }

    /// Start loading `path` to become `role`.
    pub fn load(&mut self, path: &Path, role: LoadRole, sess_trail: usize) {
        self.next_id += 1;
        let owned = path.to_path_buf();
        let run = move |p: PathBuf| load::load(&p, sess_trail).map_err(|e| format!("{e:#}"));
        let state = if self.background {
            let (tx, rx) = mpsc::channel();
            let p = owned.clone();
            std::thread::spawn(move || {
                let _ = tx.send(run(p));
            });
            State::Running(rx)
        } else {
            State::Done(Box::new(run(owned.clone())))
        };
        self.jobs.push(Job {
            id: self.next_id,
            bytes: size_on_disk(path),
            path: owned,
            role,
            started: Instant::now(),
            state,
        });
    }

    /// Start listing the datasets in `dir`.
    pub fn scan(&mut self, dir: &Path) {
        let owned = dir.to_path_buf();
        let (tx, rx) = mpsc::channel();
        if self.background {
            std::thread::spawn(move || {
                let _ = tx.send(list_datasets(&owned));
            });
        } else {
            let _ = tx.send(list_datasets(&owned));
        }
        self.scans.push(Scan {
            dir: dir.to_path_buf(),
            rx,
        });
    }

    /// Forget load `id`; its worker finishes but the result is dropped.
    pub fn cancel(&mut self, id: u64) {
        self.jobs.retain(|j| j.id != id);
    }

    /// Is anything still being read?
    pub fn busy(&self) -> bool {
        !self.jobs.is_empty() || !self.scans.is_empty()
    }

    /// The loads in progress, in the order asked for.
    pub fn loading(&self) -> Vec<LoadingInfo> {
        let mut blocked = false;
        self.jobs
            .iter()
            .map(|j| {
                let done = matches!(j.state, State::Done(_));
                // Done, but an earlier load is still running.
                let waiting = done && blocked;
                blocked |= !done;
                LoadingInfo {
                    id: j.id,
                    name: display_name(&j.path),
                    bytes: j.bytes,
                    elapsed: j.started.elapsed(),
                    role: j.role,
                    waiting,
                }
            })
            .collect()
    }

    /// Take in what the workers have finished. Returns the loads to apply, in
    /// the order they were asked for (a finished load waits for earlier ones),
    /// and the folders whose listing arrived.
    pub fn poll(&mut self) -> (Vec<Loaded>, Vec<Listed>) {
        for job in &mut self.jobs {
            if let State::Running(rx) = &job.state {
                match rx.try_recv() {
                    Ok(result) => job.state = State::Done(Box::new(result)),
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        job.state =
                            State::Done(Box::new(Err("the loader stopped unexpectedly".into())));
                    }
                }
            }
        }
        let mut ready = Vec::new();
        while self
            .jobs
            .first()
            .is_some_and(|j| matches!(j.state, State::Done(_)))
        {
            let job = self.jobs.remove(0);
            if let State::Done(result) = job.state {
                ready.push(Loaded {
                    role: job.role,
                    result: *result,
                    path: job.path,
                });
            }
        }
        let mut listings = Vec::new();
        self.scans.retain(|s| match s.rx.try_recv() {
            Ok(r) => {
                listings.push((s.dir.clone(), r));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                listings.push((s.dir.clone(), Err("the folder scan stopped".into())));
                false
            }
        });
        (ready, listings)
    }
}

/// `anat+tlrc` for `…/anat+tlrc.HEAD`, the file name otherwise.
fn display_name(path: &Path) -> String {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    );
    name.strip_suffix(".HEAD")
        .map_or(name.clone(), str::to_string)
}

/// Bytes of the dataset's data on disk: the `.BRIK` (or `.BRIK.gz`) next to a
/// `.HEAD`, or the NIfTI file itself. `None` if nothing is found.
fn size_on_disk(path: &Path) -> Option<u64> {
    let text = path.to_string_lossy();
    let base = text.strip_suffix(".HEAD").unwrap_or(&text).to_string();
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{base}.BRIK")),
        PathBuf::from(format!("{base}.BRIK.gz")),
        PathBuf::from(format!("{base}.BRIK.bz2")),
    ] {
        if candidate.is_file()
            && let Ok(m) = candidate.metadata()
            && !candidate.to_string_lossy().ends_with(".HEAD")
        {
            return Some(m.len());
        }
    }
    None
}

/// The datasets directly inside `dir`: AFNI (`.HEAD`, listed as `prefix+view`)
/// and NIfTI (`.nii`, `.nii.gz`), sorted by name, ignoring hidden files.
pub fn list_datasets(dir: &Path) -> Result<Vec<FolderEntry>, String> {
    let read = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut entries: Vec<FolderEntry> = read
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let label = if let Some(prefix) = name.strip_suffix(".HEAD") {
                prefix.to_string()
            } else if name.ends_with(".nii") || name.ends_with(".nii.gz") {
                name
            } else {
                return None;
            };
            Some(FolderEntry {
                path: dir.join(&label),
                label,
            })
        })
        .collect();
    entries.sort_by_key(|e| e.label.to_lowercase());
    entries.dedup();
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[test]
    fn a_folder_lists_afni_and_nifti_datasets_once_each() {
        let dir = crate::testutil::TempDir::new("listing");
        for name in [
            "anat+tlrc.HEAD",
            "anat+tlrc.BRIK.gz",
            "func+orig.HEAD",
            "func+orig.BRIK",
            "t1.nii.gz",
            "mask.nii",
            "notes.txt",
            ".hidden+orig.HEAD",
            "stim.1D",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        std::fs::create_dir(dir.path().join("sub+orig.HEAD")).unwrap(); // not a file
        let found = list_datasets(dir.path()).unwrap();
        let labels: Vec<&str> = found.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["anat+tlrc", "func+orig", "mask.nii", "t1.nii.gz"]);
        // The path to open for an AFNI dataset is prefix+view.
        assert_eq!(found[0].path, dir.path().join("anat+tlrc"));
        assert!(list_datasets(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn the_fixtures_folder_lists_its_afni_datasets() {
        let labels: Vec<String> = list_datasets(&fixtures())
            .unwrap()
            .into_iter()
            .map(|e| e.label)
            .collect();
        for want in [
            "tiny2+orig",
            "stat+orig",
            "obl+orig",
            "bold+orig",
            "clust+orig",
        ] {
            assert!(labels.iter().any(|l| l == want), "{want} in {labels:?}");
        }
    }

    #[test]
    fn loads_are_applied_in_the_order_asked_for_even_if_a_later_one_is_ready() {
        let mut l = Loader::new(false);
        l.load(&fixtures().join("tiny2+orig"), LoadRole::Underlay, 0);
        l.load(&fixtures().join("stat+orig"), LoadRole::Overlay, 0);
        l.load(&fixtures().join("nonexistent+orig"), LoadRole::Overlay, 0);
        let (done, _) = l.poll();
        assert_eq!(done.len(), 3);
        assert_eq!(done[0].role, LoadRole::Underlay);
        assert_eq!(done[0].result.as_ref().unwrap().name, "tiny2+orig");
        assert_eq!(done[1].result.as_ref().unwrap().name, "stat+orig");
        assert!(done[2].result.is_err());
        assert!(!l.busy());
    }

    #[test]
    fn a_finished_load_waits_for_an_earlier_one_that_is_still_running() {
        let mut l = Loader::new(false);
        // Make the first job look still running.
        let (_tx, rx) = mpsc::channel();
        l.load(&fixtures().join("tiny2+orig"), LoadRole::Underlay, 0);
        l.jobs[0].state = State::Running(rx);
        l.load(&fixtures().join("stat+orig"), LoadRole::Overlay, 0);
        let info = l.loading();
        assert_eq!(info.len(), 2);
        assert!(!info[0].waiting && info[1].waiting);
        assert!(l.poll().0.is_empty());
        assert!(l.busy());
        // Cancelling the slow one releases the other.
        let id = info[0].id;
        l.cancel(id);
        let (done, _) = l.poll();
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].role, LoadRole::Overlay);
    }

    #[test]
    fn a_threaded_load_arrives_later_without_blocking() {
        let mut l = Loader::new(true);
        l.load(&fixtures().join("bold+orig"), LoadRole::Underlay, 0);
        l.scan(&fixtures());
        let start = Instant::now();
        let (mut loaded, mut listed) = (Vec::new(), Vec::new());
        while loaded.is_empty() || listed.is_empty() {
            let (a, b) = l.poll();
            loaded.extend(a);
            listed.extend(b);
            assert!(start.elapsed().as_secs() < 20, "never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(loaded[0].result.as_ref().unwrap().nvols, 40);
        assert!(listed[0].1.as_ref().unwrap().len() > 3);
        assert!(!l.busy());
    }

    #[test]
    fn sizes_and_names_are_reported() {
        let mut l = Loader::new(false);
        l.jobs.clear();
        l.load(&fixtures().join("stat+orig.HEAD"), LoadRole::Underlay, 0);
        let info = l.loading();
        assert_eq!(info[0].name, "stat+orig");
        assert!(info[0].bytes.is_some_and(|b| b > 0));
    }
}
