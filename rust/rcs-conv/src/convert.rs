//! Converting one file: build the `.rcs`, write it atomically, report the result.

use std::path::{Path, PathBuf};

use rcs_cloud::progress::Progress;
use rcs_cloud::{build_from_source_with, BuildOptions, Error, Source, Stats};

use crate::files::default_target;
use crate::options::Options;

/// How the conversion of one file ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The file was built and written.
    Written,
    /// The file already exists and `force` is not set.
    Skipped,
    /// Read, build or write error.
    Failed,
    /// The user cancelled the operation ([`Progress::cancelled`]).
    Cancelled,
}

/// Conversion error: which path it concerns and what exactly happened.
#[derive(Debug, Clone)]
pub struct Failure {
    /// The path being reported (input, output directory or result).
    pub subject: PathBuf,
    /// Error text without the prefix.
    pub message: String,
}

impl Failure {
    /// Log line: `path: ERROR: text`.
    pub fn line(&self) -> String {
        format!("{}: ERROR: {}", self.subject.display(), self.message)
    }
}

/// Result of converting one file.
#[derive(Debug, Clone)]
pub struct Conversion {
    /// Input file.
    pub source: PathBuf,
    /// Where it was written (or would have been).
    pub target: PathBuf,
    /// Outcome.
    pub status: Status,
    /// Build statistics (only for [`Status::Written`]).
    pub stats: Option<Stats>,
    /// Error (only for [`Status::Failed`]).
    pub failure: Option<Failure>,
}

impl Conversion {
    /// Log line — matches the CLI output.
    pub fn message(&self) -> String {
        match self.status {
            Status::Written => match &self.stats {
                Some(stats) => describe(&self.source, &self.target, stats),
                None => format!("{} -> {}: written", self.source.display(), self.target.display()),
            },
            Status::Skipped => format!(
                "{} -> {}: skipped (file exists, -f required)",
                self.source.display(),
                self.target.display()
            ),
            Status::Failed => match &self.failure {
                Some(failure) => failure.line(),
                None => format!("{}: ERROR: unknown cause", self.source.display()),
            },
            Status::Cancelled => format!("{}: cancelled", self.source.display()),
        }
    }

    /// Whether the file was written.
    pub fn is_written(&self) -> bool {
        self.status == Status::Written
    }
}

/// Convert one file.
///
/// `las` selects the source: `true` — LAS/LAZ, `false` — `.pts`.
pub fn convert_one(
    source: &Path,
    options: &Options,
    build_options: &BuildOptions,
    las: bool,
    progress: &dyn Progress,
) -> Conversion {
    let target = default_target(source, options.out_dir.as_deref());
    convert_to(source, &target, options, build_options, las, progress)
}

/// Convert one file into an explicit target path (used by the FFI and the GUI
/// "Save as" flow).
pub fn convert_to(
    source: &Path,
    target: &Path,
    options: &Options,
    build_options: &BuildOptions,
    las: bool,
    progress: &dyn Progress,
) -> Conversion {
    let mut result = Conversion {
        source: source.to_path_buf(),
        target: target.to_path_buf(),
        status: Status::Failed,
        stats: None,
        failure: None,
    };

    if target.exists() && !options.force {
        result.status = Status::Skipped;
        return result;
    }

    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                result.failure = Some(Failure {
                    subject: parent.to_path_buf(),
                    message: err.to_string(),
                });
                return result;
            }
        }
    }

    let outcome: Result<Stats, Error> = if las {
        let mut reader = cloud_las::LasSource::new(source, options.las.clone());
        if options.in_memory {
            build_in_memory(&mut reader, build_options, progress, &target)
        } else {
            rcs_cloud::stream::build_streaming(&mut reader, build_options, progress, &target)
        }
    } else {
        let mut reader = cloud_pts::PtsSource::new(source);
        if options.in_memory {
            build_in_memory(&mut reader, build_options, progress, &target)
        } else {
            rcs_cloud::stream::build_streaming(&mut reader, build_options, progress, &target)
        }
    };

    let stats = match outcome {
        Ok(stats) => stats,
        Err(Error::Cancelled) => {
            result.status = Status::Cancelled;
            return result;
        }
        Err(err) => {
            result.failure = Some(Failure {
                subject: source.to_path_buf(),
                message: err.to_string(),
            });
            return result;
        }
    };

    result.status = Status::Written;
    result.stats = Some(stats);
    result
}


/// Reference path: build the whole file in memory and write it atomically.
fn build_in_memory(
    source: &mut dyn Source,
    options: &BuildOptions,
    progress: &dyn Progress,
    target: &Path,
) -> Result<Stats, Error> {
    let (blob, stats) = build_from_source_with(source, options, progress)?;
    write_atomic(target, &blob).map_err(Error::Io)?;
    Ok(stats)
}
/// Report line — the same format as in the Python reference.
pub fn describe(source: &Path, target: &Path, stats: &Stats) -> String {
    let words = if stats.metadata_words.is_empty() {
        String::new()
    } else {
        let joined: Vec<String> = stats.metadata_words.iter().map(u32::to_string).collect();
        format!("generated {} ({} records)", joined.join("/"), stats.metadata_records)
    };
    format!(
        "{} -> {}: {} points, {} tiles (depth {}, edge {:.3} m), max {}/tile, \
         {:.2} MB, type 1: {}",
        source.display(),
        target.display(),
        stats.points,
        stats.tiles,
        stats.depth,
        stats.tile_edge,
        stats.max_points_per_tile,
        stats.size as f64 / 1e6,
        words
    )
}

/// Write the file atomically: first `<name>.rcs.tmp<pid>`, then rename.
///
/// This way a cancellation or a crash never leaves a truncated `.rcs`: the
/// result path holds either a complete file or nothing.
pub fn write_atomic(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = temp_path(target);
    if let Err(err) = std::fs::write(&temp, bytes) {
        let _ = std::fs::remove_file(&temp);
        return Err(err);
    }
    if target.exists() {
        if let Err(err) = std::fs::remove_file(target) {
            let _ = std::fs::remove_file(&temp);
            return Err(err);
        }
    }
    match std::fs::rename(&temp, target) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = std::fs::remove_file(&temp);
            Err(err)
        }
    }
}

/// Temporary name next to the result (same directory, otherwise `rename` is not atomic).
fn temp_path(target: &Path) -> PathBuf {
    let mut name = target
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output.rcs".to_string());
    name.push_str(".tmp");
    name.push_str(&std::process::id().to_string());
    match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use rcs_cloud::progress::{NoProgress, Progress, Stage};

    /// Progress that records stages and can press Cancel.
    #[derive(Default)]
    struct Recorder {
        cancel: AtomicBool,
        stages: Mutex<Vec<Stage>>,
    }

    impl Progress for Recorder {
        fn stage(&self, stage: Stage, _done: u64, _total: u64) {
            let mut stages = self.stages.lock().expect("stages");
            if stages.last() != Some(&stage) {
                stages.push(stage);
            }
        }

        fn cancelled(&self) -> bool {
            self.cancel.load(Ordering::Relaxed)
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rcs-conv-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temporary directory");
        dir
    }

    fn options_for(source: &Path, out: &Path) -> Options {
        Options {
            sources: vec![source.to_string_lossy().into_owned()],
            out_dir: Some(out.to_path_buf()),
            ..Options::default()
        }
    }

    #[test]
    fn writes_atomically_and_leaves_no_temp() {
        let dir = temp_dir("atomic");
        let target = dir.join("file.rcs");
        write_atomic(&target, b"first").expect("first write");
        write_atomic(&target, b"second").expect("second write");
        assert_eq!(std::fs::read(&target).expect("read"), b"second");

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("directory")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temporary files left behind: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn converts_pts_and_reports_progress() {
        let dir = temp_dir("pts");
        let source = dir.join("tiny.pts");
        std::fs::write(&source, "3\n0 0 0\n1 1 1\n2 2 2\n").expect("input");
        let out = dir.join("out");
        let options = options_for(&source, &out);
        let build = options.build_options().expect("settings");

        let recorder = Recorder::default();
        let result = convert_one(&source, &options, &build, false, &recorder);
        assert_eq!(result.status, Status::Written, "{}", result.message());
        assert!(result.target.exists());
        assert!(result.message().contains("3 points"), "{}", result.message());

        let stages = recorder.stages.lock().expect("stages").clone();
        assert!(stages.contains(&Stage::Read), "{stages:?}");
        assert!(stages.contains(&Stage::Octree), "{stages:?}");
        assert!(stages.contains(&Stage::Tiles), "{stages:?}");
        assert!(stages.contains(&Stage::Write), "{stages:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_produces_no_file() {
        let dir = temp_dir("cancel");
        let source = dir.join("tiny.pts");
        std::fs::write(&source, "3\n0 0 0\n1 1 1\n2 2 2\n").expect("input");
        let out = dir.join("out");
        let options = options_for(&source, &out);
        let build = options.build_options().expect("settings");

        let recorder = Recorder::default();
        recorder.cancel.store(true, Ordering::Relaxed);
        let result = convert_one(&source, &options, &build, false, &recorder);
        assert_eq!(result.status, Status::Cancelled);
        assert!(!result.target.exists(), "the file must not appear");
        assert!(result.message().contains("cancelled"), "{}", result.message());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_existing_without_force_and_overwrites_with_it() {
        let dir = temp_dir("force");
        let source = dir.join("tiny.pts");
        std::fs::write(&source, "1\n0 0 0\n").expect("input");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).expect("directory");
        std::fs::write(out.join("tiny.rcs"), b"old").expect("old file");

        let options = options_for(&source, &out);
        let build = options.build_options().expect("settings");
        let result = convert_one(&source, &options, &build, false, &NoProgress);
        assert_eq!(result.status, Status::Skipped, "{}", result.message());
        assert_eq!(std::fs::read(out.join("tiny.rcs")).expect("file"), b"old");

        let forced = Options { force: true, ..options_for(&source, &out) };
        let result = convert_one(&source, &forced, &build, false, &NoProgress);
        assert_eq!(result.status, Status::Written, "{}", result.message());
        let written = std::fs::read(out.join("tiny.rcs")).expect("file");
        assert_eq!(&written[..8], b"ADOCT\0\0\0", "container signature");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_input_is_reported_as_failure() {
        let dir = temp_dir("empty");
        let source = dir.join("empty.pts");
        std::fs::write(&source, "0\n").expect("input");
        let out = dir.join("out");
        let options = options_for(&source, &out);
        let build = options.build_options().expect("settings");
        let result = convert_one(&source, &options, &build, false, &NoProgress);
        assert_eq!(result.status, Status::Failed);
        assert!(result.message().contains("ERROR"), "{}", result.message());
        assert!(!result.target.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
    /// The streaming writer and the in-memory reference must produce the same bytes.
    #[test]
    fn streaming_matches_in_memory() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../golden");
        for name in ["probe6.pts", "level_7.pts", "level_7.laz"] {
            let source = root.join(name);
            if !source.exists() {
                eprintln!("skip: no {}", source.display());
                continue;
            }
            let dir = temp_dir(&format!("stream-{}", name.replace('.', "-")));
            let stream_dir = dir.join("stream");
            let memory_dir = dir.join("memory");
            std::fs::create_dir_all(&stream_dir).expect("stream dir");
            std::fs::create_dir_all(&memory_dir).expect("memory dir");
            let guid = "{00112233-4455-6677-8899-aabbccddeeff}";

            let mut stream_options = options_for(&source, &stream_dir);
            stream_options.guid = Some(guid.to_string());
            let mut memory_options = options_for(&source, &memory_dir);
            memory_options.guid = Some(guid.to_string());
            memory_options.in_memory = true;
            let build = stream_options.build_options().expect("build options");
            let las = name.ends_with(".laz") || name.ends_with(".las");

            let streamed = convert_one(&source, &stream_options, &build, las, &NoProgress);
            let in_memory = convert_one(&source, &memory_options, &build, las, &NoProgress);
            assert_eq!(streamed.status, Status::Written, "{}", streamed.message());
            assert_eq!(in_memory.status, Status::Written, "{}", in_memory.message());
            let stem = source.file_stem().unwrap().to_string_lossy().into_owned();
            let a = std::fs::read(stream_dir.join(format!("{stem}.rcs"))).expect("stream file");
            let b = std::fs::read(memory_dir.join(format!("{stem}.rcs"))).expect("memory file");
            assert_eq!(a, b, "{name}: streaming and in-memory bytes differ");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
