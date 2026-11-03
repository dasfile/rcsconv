//! Чистая логика окна: идентификаторы контролов, размеры, подписи.
//!
//! Здесь нет Win32, поэтому модуль тестируется на любой платформе (в том числе
//! в CI на Ubuntu). Всё, что зависит от системы, живёт в [`crate::win32`].

/// Идентификаторы дочерних окон (они же `HMENU` при создании и `id` в `WM_COMMAND`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Id {
    /// Строка «rcsconv … → .rcs» и версия.
    Title = 100,
    /// Подсказка о текущем состоянии.
    Hint = 101,
    /// Кнопка «Закрыть».
    Close = 102,
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

/// Заголовок главного окна.
pub const WINDOW_TITLE: &str = "rcsconv — конвертер ReCap .rcs";
/// Имя класса окна (только ASCII — так надёжнее для `RegisterClassW`).
pub const WINDOW_CLASS: &str = "rcsconvGuiMain";
/// Подсказка о текущей фазе: до G4 конвертация ещё не подключена.
pub const HINT: &str =
    "Фаза G1: окно-каркас. Очередь файлов и конвертация — следующие шаги (docs/ROADMAP.md §4.1).";

/// Строка о версии для заголовка окна.
pub fn version_line() -> String {
    format!("rcsconv {} — .las/.laz/.pts → ReCap .rcs", env!("CARGO_PKG_VERSION"))
}

/// Пиксели, рассчитанные на 96 dpi → физические пиксели при `dpi`.
pub fn scale(px: i32, dpi: u32) -> i32 {
    let dpi = if dpi == 0 { 96 } else { dpi } as i32;
    (px * dpi + 48) / 96
}

/// Раскладка окна-каркаса: заголовок, подсказка, кнопка «Закрыть».
///
/// `width`/`height` — клиентская область в физических пикселях.
pub fn layout(width: i32, height: i32, dpi: u32) -> (Rect, Rect, Rect) {
    let margin = scale(12, dpi);
    let line = scale(22, dpi);
    let button_w = scale(120, dpi);
    let button_h = scale(28, dpi);
    let width = width.max(margin * 2 + scale(240, dpi));
    let height = height.max(margin * 2 + line * 3 + button_h);

    let title = Rect { x: margin, y: margin, w: width - margin * 2, h: line };
    let hint = Rect { x: margin, y: margin + line, w: width - margin * 2, h: line * 2 };
    let close = Rect {
        x: width - margin - button_w,
        y: height - margin - button_h,
        w: button_w,
        h: button_h,
    };
    (title, hint, close)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_keeps_96_dpi_as_is() {
        assert_eq!(scale(12, 96), 12);
        assert_eq!(scale(12, 0), 12, "0 dpi считается как 96");
        assert_eq!(scale(12, 192), 24);
        assert_eq!(scale(12, 144), 18);
    }

    #[test]
    fn layout_stays_inside_client_area() {
        for (width, height, dpi) in [(900, 600, 96), (1400, 900, 144), (600, 400, 192)] {
            let (title, hint, close) = layout(width, height, dpi);
            for rect in [title, hint, close] {
                assert!(rect.x >= 0 && rect.y >= 0, "{rect:?}");
                assert!(rect.x + rect.w <= width, "{rect:?} при ширине {width}");
                assert!(rect.y + rect.h <= height, "{rect:?} при высоте {height}");
            }
            assert!(close.y > hint.y, "кнопка ниже подсказки");
        }
    }

    #[test]
    fn ids_are_distinct_and_fit_u16() {
        assert_ne!(Id::Title.raw(), Id::Hint.raw());
        assert_ne!(Id::Hint.raw(), Id::Close.raw());
        assert!(Id::Close.raw() < 0x10000);
    }

    #[test]
    fn version_line_has_version_and_arrow() {
        let line = version_line();
        assert!(line.contains(env!("CARGO_PKG_VERSION")), "{line}");
        assert!(line.contains(".laz"), "{line}");
    }
}
