//! Чистая логика окна: очередь файлов, размеры, подписи.
//!
//! Здесь нет Win32, поэтому модуль тестируется на любой платформе (в том числе
//! в CI на Ubuntu). Всё, что зависит от системы, живёт в [`crate::win32`].

use std::path::{Path, PathBuf};

/// Расширения входа, которые принимает GUI.
pub const SUFFIXES: &[&str] = &[".las", ".laz", ".pts"];

/// Идентификаторы дочерних окон (они же `HMENU` при создании и `id` в `WM_COMMAND`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Id {
    /// Строка с именем и версией.
    Title = 100,
    /// Подсказка о состоянии.
    Hint = 101,
    /// Список файлов (`SysListView32`).
    List = 102,
    /// Кнопка «Добавить файлы…».
    AddFiles = 103,
    /// Кнопка «Добавить папку…».
    AddFolder = 104,
    /// Кнопка «Убрать».
    Remove = 105,
    /// Кнопка «Очистить».
    Clear = 106,
    /// Кнопка «Закрыть».
    Close = 107,
}

impl Id {
    /// Значение для Win32 (`HMENU` при создании, младшее слово `wParam`).
    pub fn raw(self) -> usize {
        self as u16 as usize
    }
}

/// Прямоугольник в пикселях клиентской области.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Левый край.
    pub x: i32,
    /// Верхний край.
    pub y: i32,
    /// Ширина.
    pub w: i32,
    /// Высота.
    pub h: i32,
}

/// Раскладка главного окна.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Строка с версией.
    pub title: Rect,
    /// Подсказка (сколько файлов в очереди).
    pub hint: Rect,
    /// Список файлов.
    pub list: Rect,
    /// «Добавить файлы…».
    pub add_files: Rect,
    /// «Добавить папку…».
    pub add_folder: Rect,
    /// «Убрать».
    pub remove: Rect,
    /// «Очистить».
    pub clear: Rect,
    /// «Закрыть».
    pub close: Rect,
}

/// Заголовок главного окна.
pub const WINDOW_TITLE: &str = "rcsconv — конвертер ReCap .rcs";
/// Имя класса окна (только ASCII — так надёжнее для `RegisterClassW`).
pub const WINDOW_CLASS: &str = "rcsconvGuiMain";

/// Колонки списка файлов: подпись и ширина при 96 dpi.
pub const COLUMNS: [(&str, i32); 4] =
    [("Файл", 260), ("Папка", 320), ("Размер", 90), ("Состояние", 150)];

/// Подсказка по умолчанию.
pub const HINT_EMPTY: &str =
    "Перетащите .las/.laz/.pts в окно или нажмите «Добавить файлы…». Конвертация — фаза G4.";

/// Строка о версии для заголовка окна.
pub fn version_line() -> String {
    format!("rcsconv {} — .las/.laz/.pts → ReCap .rcs", env!("CARGO_PKG_VERSION"))
}

/// Пиксели, рассчитанные на 96 dpi → физические пиксели при `dpi`.
pub fn scale(px: i32, dpi: u32) -> i32 {
    let dpi = if dpi == 0 { 96 } else { dpi } as i32;
    (px * dpi + 48) / 96
}

/// Минимальный размер клиентской области при 96 dpi (ширина, высота).
pub const MIN_SIZE_96: (i32, i32) = (560, 260);

/// Раскладка окна: заголовок, подсказка, ряд кнопок, список.
///
/// `width`/`height` — клиентская область в физических пикселях; размер
/// поднимается до [`MIN_SIZE_96`] (окно не даёт сжать себя меньше — `WM_GETMINMAXINFO`).
pub fn layout(width: i32, height: i32, dpi: u32) -> Layout {
    let margin = scale(12, dpi);
    let line = scale(20, dpi);
    let button_h = scale(28, dpi);
    let gap = scale(8, dpi);

    let width = width.max(scale(MIN_SIZE_96.0, dpi));
    let height = height.max(scale(MIN_SIZE_96.1, dpi));

    let title = Rect { x: margin, y: margin, w: width - margin * 2, h: line };
    let hint = Rect { x: margin, y: margin + line, w: width - margin * 2, h: line };

    let buttons_y = hint.y + hint.h + gap / 2;
    let add_files = Rect { x: margin, y: buttons_y, w: scale(150, dpi), h: button_h };
    let add_folder = Rect {
        x: add_files.x + add_files.w + gap,
        y: buttons_y,
        w: scale(150, dpi),
        h: button_h,
    };
    let remove = Rect {
        x: add_folder.x + add_folder.w + gap,
        y: buttons_y,
        w: scale(100, dpi),
        h: button_h,
    };
    let clear = Rect {
        x: remove.x + remove.w + gap,
        y: buttons_y,
        w: scale(100, dpi),
        h: button_h,
    };
    let close = Rect {
        x: width - margin - scale(120, dpi),
        y: buttons_y,
        w: scale(120, dpi),
        h: button_h,
    };

    let list_top = buttons_y + button_h + gap;
    let list = Rect {
        x: margin,
        y: list_top,
        w: width - margin * 2,
        h: (height - margin - list_top).max(scale(80, dpi)),
    };

    Layout { title, hint, list, add_files, add_folder, remove, clear, close }
}

/// Состояние файла в очереди.
///
/// Все варианты, кроме [`EntryStatus::Waiting`], появятся в G4 (конвертация);
/// держим их здесь, чтобы колонка «Состояние» была готова.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryStatus {
    /// Ждёт конвертации.
    Waiting,
    /// Конвертируется прямо сейчас.
    Running,
    /// Файл записан.
    Written,
    /// Пропущен (файл уже есть).
    Skipped,
    /// Ошибка.
    Failed,
    /// Отменён пользователем.
    Cancelled,
}

impl EntryStatus {
    /// Подпись для колонки «Состояние».
    pub fn label(self) -> &'static str {
        match self {
            EntryStatus::Waiting => "в очереди",
            EntryStatus::Running => "идёт…",
            EntryStatus::Written => "готово",
            EntryStatus::Skipped => "пропущен",
            EntryStatus::Failed => "ошибка",
            EntryStatus::Cancelled => "отменён",
        }
    }
}

/// Файл в очереди.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Путь к файлу.
    pub path: PathBuf,
    /// Размер в байтах (0, если не удалось узнать).
    pub size: u64,
    /// Состояние.
    pub status: EntryStatus,
}

/// Очередь файлов на конвертацию.
///
/// Дубликаты отсекаются по каноническому пути, каталоги разворачиваются в файлы
/// с нужными расширениями — то же делает [`rcs_conv::expand_sources`] в CLI,
/// поэтому набор файлов в GUI и в CLI совпадает.
#[derive(Debug, Default, Clone)]
pub struct Queue {
    entries: Vec<Entry>,
}

impl Queue {
    /// Пустая очередь.
    pub fn new() -> Queue {
        Queue::default()
    }

    /// Число файлов.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Пуста ли очередь.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Файлы очереди.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Пути файлов очереди (понадобится в G4 для запуска конвертации).
    #[allow(dead_code)]
    pub fn paths(&self) -> Vec<PathBuf> {
        self.entries.iter().map(|entry| entry.path.clone()).collect()
    }

    /// Добавить файлы, каталоги и маски; вернуть число новых файлов.
    pub fn add(&mut self, patterns: &[PathBuf], recursive: bool) -> usize {
        let as_text: Vec<String> =
            patterns.iter().map(|path| path.to_string_lossy().into_owned()).collect();
        let expanded = rcs_conv::expand_sources(&as_text, SUFFIXES, recursive);
        let mut added = 0;
        for path in expanded {
            if !path.is_file() {
                continue;
            }
            let key = canonical_key(&path);
            if self.entries.iter().any(|entry| canonical_key(&entry.path) == key) {
                continue;
            }
            let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            self.entries.push(Entry { path, size, status: EntryStatus::Waiting });
            added += 1;
        }
        added
    }

    /// Убрать файл по индексу; `true`, если что-то убрали.
    pub fn remove(&mut self, index: usize) -> bool {
        if index < self.entries.len() {
            self.entries.remove(index);
            true
        } else {
            false
        }
    }

    /// Очистить очередь.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Сменить состояние файла (G4).
    #[allow(dead_code)]
    pub fn set_status(&mut self, index: usize, status: EntryStatus) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.status = status;
        }
    }

    /// Сбросить состояния всех файлов в «в очереди» (G4).
    #[allow(dead_code)]
    pub fn reset_statuses(&mut self) {
        for entry in &mut self.entries {
            entry.status = EntryStatus::Waiting;
        }
    }

    /// Суммарный размер файлов, байт.
    pub fn total_size(&self) -> u64 {
        self.entries.iter().map(|entry| entry.size).sum()
    }
}

/// Ключ для отсечения дубликатов: канонический путь, иначе исходный.
fn canonical_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Размер в байтах → человекочитаемая строка (`1.20 МБ`).
pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    if value < KB {
        format!("{bytes} Б")
    } else if value < KB * KB {
        format!("{:.1} КБ", value / KB)
    } else if value < KB * KB * KB {
        format!("{:.2} МБ", value / (KB * KB))
    } else {
        format!("{:.2} ГБ", value / (KB * KB * KB))
    }
}

/// Строка подсказки: сколько файлов и какой суммарный размер.
pub fn queue_hint(queue: &Queue) -> String {
    if queue.is_empty() {
        return HINT_EMPTY.to_string();
    }
    format!(
        "В очереди {} файл(ов), {} — конвертация появится в фазе G4.",
        queue.len(),
        format_size(queue.total_size())
    )
}

/// Разобрать буфер `GetOpenFileNameW` с `OFN_ALLOWMULTISELECT`.
///
/// Win32 пишет либо один полный путь, либо «каталог\0имя1\0имя2\0\0».
/// Функция чистая (без Win32), поэтому проверяется обычным тестом.
pub fn parse_open_file_buffer(buffer: &[u16]) -> Vec<PathBuf> {
    let end_of_first = buffer.iter().position(|&value| value == 0).unwrap_or(buffer.len());
    if end_of_first == 0 {
        return Vec::new();
    }
    let first = String::from_utf16_lossy(&buffer[..end_of_first]);
    let rest_start = end_of_first + 1;
    if rest_start >= buffer.len() || buffer[rest_start] == 0 {
        return vec![PathBuf::from(first)];
    }
    let dir = PathBuf::from(first);
    let mut files = Vec::new();
    let mut index = rest_start;
    while index < buffer.len() && buffer[index] != 0 {
        let end = buffer[index..]
            .iter()
            .position(|&value| value == 0)
            .map(|offset| index + offset)
            .unwrap_or(buffer.len());
        files.push(dir.join(String::from_utf16_lossy(&buffer[index..end])));
        index = end + 1;
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rcs-gui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("временный каталог");
        dir
    }

    #[test]
    fn scale_keeps_96_dpi_as_is() {
        assert_eq!(scale(12, 96), 12);
        assert_eq!(scale(12, 0), 12, "0 dpi считается как 96");
        assert_eq!(scale(12, 192), 24);
        assert_eq!(scale(12, 144), 18);
    }

    #[test]
    fn layout_stays_inside_client_area() {
        for (width, height, dpi) in [(900, 600, 96), (1400, 900, 144), (1200, 800, 192)] {
            let effective_width = width.max(scale(MIN_SIZE_96.0, dpi));
            let effective_height = height.max(scale(MIN_SIZE_96.1, dpi));
            let layout = layout(width, height, dpi);
            for rect in [
                layout.title,
                layout.hint,
                layout.list,
                layout.add_files,
                layout.add_folder,
                layout.remove,
                layout.clear,
                layout.close,
            ] {
                assert!(rect.x >= 0 && rect.y >= 0, "{rect:?}");
                assert!(rect.x + rect.w <= effective_width, "{rect:?} при ширине {width}");
                assert!(rect.y + rect.h <= effective_height, "{rect:?} при высоте {height}");
            }
            assert!(layout.list.y > layout.hint.y, "список ниже подсказки");
            assert!(layout.close.x + layout.close.w <= effective_width, "«Закрыть» внутри окна");
            assert!(layout.clear.x + layout.clear.w <= effective_width, "«Очистить» внутри окна");
        }
    }

    #[test]
    fn layout_survives_tiny_window() {
        // Окно меньше минимума: раскладка поднимает размер до MIN_SIZE_96.
        let layout = layout(10, 10, 96);
        assert!(layout.list.w > 0 && layout.list.h > 0, "{:?}", layout.list);
        assert!(layout.add_files.w > 0);
    }

    #[test]
    fn ids_are_distinct_and_fit_u16() {
        let ids = [
            Id::Title,
            Id::Hint,
            Id::List,
            Id::AddFiles,
            Id::AddFolder,
            Id::Remove,
            Id::Clear,
            Id::Close,
        ];
        for (index, id) in ids.iter().enumerate() {
            assert!(id.raw() < 0x10000);
            assert!(!ids[..index].contains(id), "id {id:?} повторяется");
        }
    }

    #[test]
    fn version_line_has_version_and_arrow() {
        let line = version_line();
        assert!(line.contains(env!("CARGO_PKG_VERSION")), "{line}");
        assert!(line.contains(".laz"), "{line}");
    }

    #[test]
    fn format_size_units() {
        assert_eq!(format_size(512), "512 Б");
        assert_eq!(format_size(2048), "2.0 КБ");
        assert_eq!(format_size(5 * 1024 * 1024), "5.00 МБ");
    }

    #[test]
    fn open_file_buffer_single_path() {
        let mut buffer: Vec<u16> = "D:\\scan\\one.laz".encode_utf16().collect();
        buffer.push(0);
        buffer.push(0);
        assert_eq!(parse_open_file_buffer(&buffer), vec![PathBuf::from("D:\\scan\\one.laz")]);
        assert!(parse_open_file_buffer(&[0, 0]).is_empty(), "пустой буфер — ничего");
    }

    #[test]
    fn open_file_buffer_multi_select() {
        // Каталог, затем имена файлов, затем пустая строка.
        let mut buffer: Vec<u16> = "D:\\scan".encode_utf16().collect();
        for name in ["a.laz", "b.las"] {
            buffer.push(0);
            buffer.extend(name.encode_utf16());
        }
        buffer.push(0);
        buffer.push(0);
        assert_eq!(
            parse_open_file_buffer(&buffer),
            vec![PathBuf::from("D:\\scan\\a.laz"), PathBuf::from("D:\\scan\\b.las")]
        );
    }

    #[test]
    fn queue_adds_files_directories_and_skips_duplicates() {
        let dir = temp_dir("queue");
        std::fs::create_dir_all(dir.join("sub")).expect("подкаталог");
        for name in ["a.laz", "b.LAS", "c.pts", "notes.txt"] {
            std::fs::write(dir.join(name), b"0123456789").expect("файл");
        }
        std::fs::write(dir.join("sub").join("d.laz"), b"01234").expect("файл");

        let mut queue = Queue::new();
        assert_eq!(queue.add(&[dir.clone()], false), 3, "без обхода: файлы каталога");
        assert_eq!(queue.len(), 3);

        assert_eq!(queue.add(&[dir.clone()], false), 0, "дубликаты не добавляются");
        assert_eq!(queue.len(), 3);

        assert_eq!(queue.add(&[dir.clone()], true), 1, "с обходом подкаталогов");
        assert_eq!(queue.len(), 4);
        assert!(queue.total_size() > 0);
        assert!(queue_hint(&queue).contains('4'), "{}", queue_hint(&queue));

        assert!(queue.remove(0));
        assert_eq!(queue.len(), 3);
        assert!(!queue.remove(99));

        queue.clear();
        assert!(queue.is_empty());
        assert_eq!(queue_hint(&queue), HINT_EMPTY);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn queue_statuses_are_reset() {
        let dir = temp_dir("status");
        let file = dir.join("a.laz");
        std::fs::write(&file, b"x").expect("файл");

        let mut queue = Queue::new();
        queue.add(&[file], false);
        queue.set_status(0, EntryStatus::Written);
        assert_eq!(queue.entries()[0].status, EntryStatus::Written);
        queue.reset_statuses();
        assert_eq!(queue.entries()[0].status, EntryStatus::Waiting);
        assert_eq!(EntryStatus::Waiting.label(), "в очереди");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
