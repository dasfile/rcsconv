//! Прогресс и отмена длинных операций.
//!
//! Нужны GUI (`rcsconv-gui`): прогресс-бар по этапам и кнопка «Отмена».
//! CLI передаёт [`NoProgress`], поэтому ни вывод, ни байты эталонов не меняются.

/// Этап сборки `.rcs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Чтение источника (`.las/.laz/.pts`).
    Read,
    /// Выбор глубины и раскладка точек по ячейкам.
    Octree,
    /// Запись узлов (`type 3`) и тел точек (`type 11`).
    Tiles,
    /// Сборка контейнера: заголовок, директория, блоки.
    Write,
}

impl Stage {
    /// Название этапа для интерфейса.
    pub fn title(self) -> &'static str {
        match self {
            Stage::Read => "чтение",
            Stage::Octree => "октодерево",
            Stage::Tiles => "узлы",
            Stage::Write => "запись",
        }
    }
}

/// Приёмник отчётов о ходе сборки.
///
/// Реализации должны быть дешёвыми: `stage` вызывается часто (несколько раз
/// на миллион точек), а `cancelled` — ещё чаще, на каждой проверке.
pub trait Progress: Send + Sync {
    /// Текущий этап: сделано `done` из `total` (`total = 0` — всего неизвестно).
    fn stage(&self, stage: Stage, done: u64, total: u64) {
        let _ = (stage, done, total);
    }

    /// Запрошена ли отмена. Проверяется в точках останова сборки.
    fn cancelled(&self) -> bool {
        false
    }
}

/// Приёмник-пустышка: ни отчётов, ни отмены (CLI, тесты, паритетные прогоны).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoProgress;

impl Progress for NoProgress {}
