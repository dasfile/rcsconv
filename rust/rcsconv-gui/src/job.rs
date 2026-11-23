//! Background conversion thread: queue -> `rcs_conv::convert_one`.
//!
//! The UI never blocks: the thread reports progress through [`Shared`] (the
//! `rcs_conv::Progress` trait), the window polls it on a timer and takes finished
//! results. Cancellation is a flag in the same `Shared`: `convert_one` checks it
//! at build stages and the file is written atomically, so a cancelled `.rcs` never
//! remains on disk.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use rcs_conv::{convert_one, Conversion, Progress, Stage, Status};

use crate::params::Settings;

/// Job state shared by the thread and the window.
pub struct Shared {
    stage: Mutex<(Stage, u64, u64)>,
    cancel: AtomicBool,
    results: Mutex<Vec<Conversion>>,
    current: AtomicUsize,
    finished: AtomicBool,
}

impl Default for Shared {
    fn default() -> Self {
        Shared {
            stage: Mutex::new((Stage::Read, 0, 0)),
            cancel: AtomicBool::new(false),
            results: Mutex::new(Vec::new()),
            current: AtomicUsize::new(0),
            finished: AtomicBool::new(false),
        }
    }
}

impl Progress for Shared {
    fn stage(&self, stage: Stage, done: u64, total: u64) {
        if let Ok(mut slot) = self.stage.lock() {
            *slot = (stage, done, total);
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

impl Shared {
    /// New job state.
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared::default())
    }

    /// Ask to cancel the conversion.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Current stage: `(stage, done, total)`.
    pub fn progress(&self) -> (Stage, u64, u64) {
        self.stage.lock().map(|slot| *slot).unwrap_or((Stage::Read, 0, 0))
    }

    /// Finished results (a copy; the window reads them on the UI thread).
    pub fn results(&self) -> Vec<Conversion> {
        self.results.lock().map(|items| items.clone()).unwrap_or_default()
    }

    /// The job is finished (successfully or cancelled).
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }

    pub(crate) fn mark_finished(&self) {
        self.finished.store(true, Ordering::Relaxed);
    }

    fn push(&self, result: Conversion) {
        if let Ok(mut items) = self.results.lock() {
            items.push(result);
        }
    }
}

/// Convert the files one by one (synchronously — this is the thread and test body).
pub fn run_jobs(paths: Vec<PathBuf>, settings: Settings, shared: &Shared) {
    // `convert_one` does not use `Options::sources`, but validation requires a
    // non-empty list — pass a placeholder.
    let Ok(options) = settings.to_options(vec![".".to_string()]) else {
        shared.mark_finished();
        return;
    };
    let Ok(build) = options.build_options() else {
        shared.mark_finished();
        return;
    };

    for (index, path) in paths.iter().enumerate() {
        if shared.cancelled() {
            break;
        }
        shared.current.store(index, Ordering::Relaxed);
        let result = convert_one(path, &options, &build, is_las(path), shared);
        let cancelled = result.status == Status::Cancelled;
        shared.push(result);
        if cancelled {
            break;
        }
    }
    shared.mark_finished();
}

/// Start the conversion in a background thread.
///
/// A panic inside the thread must not take the window down: it is caught and the
/// job is marked finished (one reason for `panic = "unwind"` in the release profile).
pub fn spawn(paths: Vec<PathBuf>, settings: Settings, shared: Arc<Shared>) -> JoinHandle<()> {
    thread::spawn(move || {
        let guard = shared.clone();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            run_jobs(paths, settings, &shared);
        }));
        if outcome.is_err() {
            guard.mark_finished();
        }
    })
}

/// LAS/LAZ source? (`false` — `.pts`).
fn is_las(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("las") || value.eq_ignore_ascii_case("laz"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rcs-gui-job-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temporary directory");
        dir
    }

    fn write_pts(path: &Path) {
        std::fs::write(path, "3\n0 0 0\n1 1 1\n2 2 2\n").expect("input");
    }

    #[test]
    fn converts_all_files_and_reports_finished() {
        let dir = temp_dir("all");
        let first = dir.join("a.pts");
        let second = dir.join("b.pts");
        write_pts(&first);
        write_pts(&second);
        let out = dir.join("out");

        let settings = Settings { out_dir: out.display().to_string(), ..Settings::default() };
        let shared = Shared::new();
        run_jobs(vec![first, second], settings, &shared);

        assert!(shared.is_finished());
        let results = shared.results();
        assert_eq!(results.len(), 2, "{results:?}");
        assert!(results.iter().all(|result| result.status == Status::Written));
        assert!(out.join("a.rcs").exists() && out.join("b.rcs").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_stops_before_any_file() {
        let dir = temp_dir("cancel");
        let file = dir.join("a.pts");
        write_pts(&file);
        let out = dir.join("out");

        let shared = Shared::new();
        shared.cancel();
        run_jobs(
            vec![file],
            Settings { out_dir: out.display().to_string(), ..Settings::default() },
            &shared,
        );

        assert!(shared.is_finished());
        assert!(shared.results().is_empty(), "files are not converted after cancellation");
        assert!(!out.join("a.rcs").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The GUI path (`Settings` -> `Options`) and the CLI path (`Options::parse`)
    /// produce a byte-identical `.rcs` on a real cloud (requires `golden/`).
    #[test]
    fn gui_and_cli_produce_identical_bytes() {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../golden/level_7.laz");
        if !source.exists() {
            eprintln!("skipped: no {}", source.display());
            return;
        }
        let dir = temp_dir("parity");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).expect("directory");
        let guid = "{00112233-4455-6677-8899-aabbccddeeff}";

        let settings = Settings {
            out_dir: out.display().to_string(),
            guid: guid.to_string(),
            ..Settings::default()
        };
        let shared = Shared::new();
        run_jobs(vec![source.clone()], settings, &shared);
        let results = shared.results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, Status::Written, "{}", results[0].message());

        let cli_dir = dir.join("cli");
        std::fs::create_dir_all(&cli_dir).expect("directory");
        let args: Vec<String> = vec![
            source.display().to_string(),
            "-o".to_string(),
            cli_dir.display().to_string(),
            "-g".to_string(),
            guid.to_string(),
        ];
        let options = rcs_conv::Options::parse(&args, true).expect("CLI parse");
        let build = options.build_options().expect("settings");
        let result = convert_one(&source, &options, &build, true, &rcs_conv::NoProgress);
        assert_eq!(result.status, Status::Written);

        let gui_bytes = std::fs::read(out.join("level_7.rcs")).expect("GUI file");
        let cli_bytes = std::fs::read(cli_dir.join("level_7.rcs")).expect("CLI file");
        assert_eq!(gui_bytes.len(), cli_bytes.len(), "sizes match");
        assert_eq!(gui_bytes, cli_bytes, "GUI and CLI produce the same file");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
