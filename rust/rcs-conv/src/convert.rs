//! Конвертация одного файла: сборка `.rcs`, атомарная запись, результат.

use std::path::{Path, PathBuf};

use rcs_cloud::progress::Progress;
use rcs_cloud::{build_from_source_with, BuildOptions, Error, Stats};

use crate::files::default_target;
use crate::options::Options;

/// Чем закончилась конвертация одного файла.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Файл собран и записан.
    Written,
    /// Файл уже есть, а `force` не задан.
    Skipped,
    /// Ошибка чтения, сборки или записи.
    Failed,
    /// Пользователь отменил операцию ([`Progress::cancelled`]).
    Cancelled,
}

/// Ошибка конвертации: к какому пути относится и что именно случилось.
#[derive(Debug, Clone)]
pub struct Failure {
    /// Путь, о котором сообщается (вход, каталог вывода или результат).
    pub subject: PathBuf,
    /// Текст ошибки без префикса.
    pub message: String,
}

impl Failure {
    /// Строка для журнала: `путь: ОШИБКА: текст`.
    pub fn line(&self) -> String {
        format!("{}: ОШИБКА: {}", self.subject.display(), self.message)
    }
}

/// Результат конвертации одного файла.
#[derive(Debug, Clone)]
pub struct Conversion {
    /// Входной файл.
    pub source: PathBuf,
    /// Куда писали (или писали бы).
    pub target: PathBuf,
    /// Итог.
    pub status: Status,
    /// Статистика сборки (только при [`Status::Written`]).
    pub stats: Option<Stats>,
    /// Ошибка (только при [`Status::Failed`]).
    pub failure: Option<Failure>,
}

impl Conversion {
    /// Строка для журнала — совпадает с выводом CLI.
    pub fn message(&self) -> String {
        match self.status {
            Status::Written => match &self.stats {
                Some(stats) => describe(&self.source, &self.target, stats),
                None => format!("{} → {}: записано", self.source.display(), self.target.display()),
            },
            Status::Skipped => format!(
                "{} → {}: пропущено (файл есть, нужен -f)",
                self.source.display(),
                self.target.display()
            ),
            Status::Failed => match &self.failure {
                Some(failure) => failure.line(),
                None => format!("{}: ОШИБКА: неизвестная причина", self.source.display()),
            },
            Status::Cancelled => format!("{}: отменено", self.source.display()),
        }
    }

    /// Записан ли файл.
    pub fn is_written(&self) -> bool {
        self.status == Status::Written
    }
}

/// Сконвертировать один файл.
///
/// `las` выбирает источник: `true` — LAS/LAZ, `false` — `.pts`.
pub fn convert_one(
    source: &Path,
    options: &Options,
    build_options: &BuildOptions,
    las: bool,
    progress: &dyn Progress,
) -> Conversion {
    let target = default_target(source, options.out_dir.as_deref());
    let mut result = Conversion {
        source: source.to_path_buf(),
        target: target.clone(),
        status: Status::Failed,
        stats: None,
        failure: None,
    };

    if target.exists() && !options.force {
        result.status = Status::Skipped;
        return result;
    }

    let built = if las {
        let mut reader = cloud_las::LasSource::new(source, options.las.clone());
        build_from_source_with(&mut reader, build_options, progress)
    } else {
        let mut reader = cloud_pts::PtsSource::new(source);
        build_from_source_with(&mut reader, build_options, progress)
    };

    let (blob, stats) = match built {
        Ok(built) => built,
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

    if let Err(err) = write_atomic(&target, &blob) {
        result.failure = Some(Failure {
            subject: target.clone(),
            message: err.to_string(),
        });
        return result;
    }

    result.status = Status::Written;
    result.stats = Some(stats);
    result
}

/// Строка отчёта — тот же формат, что в Python-эталоне.
pub fn describe(source: &Path, target: &Path, stats: &Stats) -> String {
    let words = if stats.metadata_words.is_empty() {
        String::new()
    } else {
        let joined: Vec<String> = stats.metadata_words.iter().map(u32::to_string).collect();
        format!("generated {} ({} записей)", joined.join("/"), stats.metadata_records)
    };
    format!(
        "{} → {}: точек {}, узлов {} (глубина {}, ребро {:.3} м), макс. {}/узел, \
         {:.2} МБ, type 1: {}",
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

/// Записать файл атомарно: сначала `<имя>.rcs.tmp<pid>`, потом переименование.
///
/// Так отмена и падение не оставляют после себя обрезанный `.rcs`: на месте
/// результата либо готовый файл, либо ничего.
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

/// Временное имя рядом с результатом (в том же каталоге — иначе `rename` не атомарен).
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

    /// Прогресс, который запоминает этапы и умеет «нажать Отмена».
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
        std::fs::create_dir_all(&dir).expect("временный каталог");
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
        write_atomic(&target, b"first").expect("первая запись");
        write_atomic(&target, b"second").expect("вторая запись");
        assert_eq!(std::fs::read(&target).expect("чтение"), b"second");

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("каталог")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "временные файлы остались: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn converts_pts_and_reports_progress() {
        let dir = temp_dir("pts");
        let source = dir.join("tiny.pts");
        std::fs::write(&source, "3\n0 0 0\n1 1 1\n2 2 2\n").expect("вход");
        let out = dir.join("out");
        let options = options_for(&source, &out);
        let build = options.build_options().expect("параметры");

        let recorder = Recorder::default();
        let result = convert_one(&source, &options, &build, false, &recorder);
        assert_eq!(result.status, Status::Written, "{}", result.message());
        assert!(result.target.exists());
        assert!(result.message().contains("точек 3"), "{}", result.message());

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
        std::fs::write(&source, "3\n0 0 0\n1 1 1\n2 2 2\n").expect("вход");
        let out = dir.join("out");
        let options = options_for(&source, &out);
        let build = options.build_options().expect("параметры");

        let recorder = Recorder::default();
        recorder.cancel.store(true, Ordering::Relaxed);
        let result = convert_one(&source, &options, &build, false, &recorder);
        assert_eq!(result.status, Status::Cancelled);
        assert!(!result.target.exists(), "файл не должен появиться");
        assert!(result.message().contains("отменено"), "{}", result.message());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_existing_without_force_and_overwrites_with_it() {
        let dir = temp_dir("force");
        let source = dir.join("tiny.pts");
        std::fs::write(&source, "1\n0 0 0\n").expect("вход");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).expect("каталог");
        std::fs::write(out.join("tiny.rcs"), b"old").expect("старый файл");

        let options = options_for(&source, &out);
        let build = options.build_options().expect("параметры");
        let result = convert_one(&source, &options, &build, false, &NoProgress);
        assert_eq!(result.status, Status::Skipped, "{}", result.message());
        assert_eq!(std::fs::read(out.join("tiny.rcs")).expect("файл"), b"old");

        let forced = Options { force: true, ..options_for(&source, &out) };
        let result = convert_one(&source, &forced, &build, false, &NoProgress);
        assert_eq!(result.status, Status::Written, "{}", result.message());
        let written = std::fs::read(out.join("tiny.rcs")).expect("файл");
        assert_eq!(&written[..8], b"ADOCT\0\0\0", "сигнатура контейнера");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_input_is_reported_as_failure() {
        let dir = temp_dir("empty");
        let source = dir.join("empty.pts");
        std::fs::write(&source, "0\n").expect("вход");
        let out = dir.join("out");
        let options = options_for(&source, &out);
        let build = options.build_options().expect("параметры");
        let result = convert_one(&source, &options, &build, false, &NoProgress);
        assert_eq!(result.status, Status::Failed);
        assert!(result.message().contains("ОШИБКА"), "{}", result.message());
        assert!(!result.target.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
