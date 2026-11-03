//! Фоновый поток конвертации: очередь → `rcs_conv::convert_one`.
//!
//! UI не блокируется: поток сообщает прогресс через [`Shared`] (трейт
//! `rcs_conv::Progress`), окно опрашивает его по таймеру и забирает готовые
//! результаты. Отмена — флаг в том же `Shared`: `convert_one` проверяет его на
//! этапах сборки, а файл пишется атомарно, поэтому отменённый `.rcs` не остаётся.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use rcs_conv::{convert_one, Conversion, Progress, Stage, Status};

use crate::params::Settings;

/// Состояние задания, разделяемое потоком и окном.
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
    /// Новое состояние задания.
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared::default())
    }

    /// Попросить отменить конвертацию.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Текущий этап: `(этап, сделано, всего)`.
    pub fn progress(&self) -> (Stage, u64, u64) {
        self.stage.lock().map(|slot| *slot).unwrap_or((Stage::Read, 0, 0))
    }

    /// Готовые результаты (копия; окно читает их в UI-потоке).
    pub fn results(&self) -> Vec<Conversion> {
        self.results.lock().map(|items| items.clone()).unwrap_or_default()
    }

    /// Задание завершено (успешно или отменено).
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

/// Сконвертировать файлы по очереди (синхронно — это тело потока и тестов).
pub fn run_jobs(paths: Vec<PathBuf>, settings: Settings, shared: &Shared) {
    // `convert_one` не использует `Options::sources`, но валидация требует
    // непустой список — передаём заглушку.
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

/// Запустить конвертацию в фоновом потоке.
///
/// Паника внутри потока не должна уносить окно: она перехватывается, а задание
/// помечается завершённым (одна из причин `panic = "unwind"` в профиле release).
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

/// Источник LAS/LAZ? (`false` — `.pts`).
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
        std::fs::create_dir_all(&dir).expect("временный каталог");
        dir
    }

    fn write_pts(path: &Path) {
        std::fs::write(path, "3\n0 0 0\n1 1 1\n2 2 2\n").expect("вход");
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
        assert!(shared.results().is_empty(), "при отмене файлы не конвертируются");
        assert!(!out.join("a.rcs").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GUI-путь (`Settings` → `Options`) и CLI-путь (`Options::parse`) дают
    /// побайтово одинаковый `.rcs` на реальном облаке (нужен `golden/`).
    #[test]
    fn gui_and_cli_produce_identical_bytes() {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../golden/level_7.laz");
        if !source.exists() {
            eprintln!("пропуск: нет {}", source.display());
            return;
        }
        let dir = temp_dir("parity");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).expect("каталог");
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
        std::fs::create_dir_all(&cli_dir).expect("каталог");
        let args: Vec<String> = vec![
            source.display().to_string(),
            "-o".to_string(),
            cli_dir.display().to_string(),
            "-g".to_string(),
            guid.to_string(),
        ];
        let options = rcs_conv::Options::parse(&args, true).expect("разбор CLI");
        let build = options.build_options().expect("параметры");
        let result = convert_one(&source, &options, &build, true, &rcs_conv::NoProgress);
        assert_eq!(result.status, Status::Written);

        let gui_bytes = std::fs::read(out.join("level_7.rcs")).expect("файл GUI");
        let cli_bytes = std::fs::read(cli_dir.join("level_7.rcs")).expect("файл CLI");
        assert_eq!(gui_bytes.len(), cli_bytes.len(), "размеры совпадают");
        assert_eq!(gui_bytes, cli_bytes, "GUI и CLI дают один и тот же файл");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
