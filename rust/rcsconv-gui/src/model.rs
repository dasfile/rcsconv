//! Pure window logic: file queue, sizes, labels.
//!
//! There is no Win32 here, so the module is tested on any platform (including
//! CI on Ubuntu). Everything system-dependent lives in [`crate::win32`].

use std::path::{Path, PathBuf};

/// Input extensions accepted by the GUI.
pub const SUFFIXES: &[&str] = &[".las", ".laz", ".pts"];

/// Child window ids (also `HMENU` at creation and `id` in `WM_COMMAND`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Id {
    /// Name and version line.
    Title = 100,
    /// Status hint.
    Hint = 101,
    /// File list (`SysListView32`).
    List = 102,
    /// "Add files..." button.
    AddFiles = 103,
    /// "Add folder..." button.
    AddFolder = 104,
    /// "Remove" button.
    Remove = 105,
    /// "Clear" button.
    Clear = 106,
    /// "Close" button.
    Close = 107,
    /// "Output folder" field.
    OutDir = 108,
    /// "Browse..." button for the output folder.
    BrowseOut = 109,
    /// "Scale" field.
    Units = 110,
    /// "Recenter" combo box.
    Recenter = 111,
    /// "Points per tile" field.
    MaxTile = 112,
    /// "GUID" field.
    Guid = 113,
    /// "Colour" check box.
    ColorCheck = 114,
    /// "Intensity" check box.
    IntensityCheck = 115,
    /// "Overwrite" check box.
    ForceCheck = 116,
    /// "Include subfolders" check box.
    RecursiveCheck = 117,
    /// "Convert" / "Cancel" button.
    Convert = 118,
    /// "Open folder" button.
    OpenOut = 119,
    /// "Output folder" label.
    LabelOutDir = 120,
    /// "Scale" label.
    LabelUnits = 121,
    /// "Recenter" label.
    LabelRecenter = 122,
    /// "Points per tile" label.
    LabelMaxTile = 123,
    /// "GUID" label.
    LabelGuid = 124,
    /// Progress bar.
    Progress = 125,
}

impl Id {
    /// Value for Win32 (`HMENU` at creation, low word of `wParam`).
    pub fn raw(self) -> usize {
        self as u16 as usize
    }
}

/// Rectangle in client-area pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
}

/// Main window layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Version line.
    pub title: Rect,
    /// Hint (how many files are queued).
    pub hint: Rect,
    /// File list.
    pub list: Rect,
    /// "Output folder" label.
    pub label_out_dir: Rect,
    /// Output folder field.
    pub out_dir: Rect,
    /// "Browse..." button.
    pub browse_out: Rect,
    /// "Scale" label.
    pub label_units: Rect,
    /// Scale field.
    pub units: Rect,
    /// "Recenter" label.
    pub label_recenter: Rect,
    /// Recenter combo box.
    pub recenter: Rect,
    /// "Points per tile" label.
    pub label_max_tile: Rect,
    /// Points per tile field.
    pub max_tile: Rect,
    /// "GUID" label.
    pub label_guid: Rect,
    /// GUID field.
    pub guid: Rect,
    /// "Colour" check box.
    pub color: Rect,
    /// "Intensity" check box.
    pub intensity: Rect,
    /// "Overwrite" check box.
    pub force: Rect,
    /// "Include subfolders" check box.
    pub recursive: Rect,
    /// "Convert" / "Cancel".
    pub convert: Rect,
    /// "Open folder".
    pub open_out: Rect,
    /// Progress bar.
    pub progress: Rect,
    /// "Add files...".
    pub add_files: Rect,
    /// "Add folder...".
    pub add_folder: Rect,
    /// "Remove".
    pub remove: Rect,
    /// "Clear".
    pub clear: Rect,
    /// "Close".
    pub close: Rect,
}

/// Main window title.
pub const WINDOW_TITLE: &str = "rcsconv — ReCap .rcs converter";
/// Window class name (ASCII only — safer for `RegisterClassW`).
pub const WINDOW_CLASS: &str = "rcsconvGuiMain";

/// File list columns: label and width at 96 dpi.
pub const COLUMNS: [(&str, i32); 4] =
    [("File", 260), ("Folder", 320), ("Size", 90), ("Status", 150)];

/// Default hint.
pub const HINT_EMPTY: &str =
    "Drag .las/.laz/.pts into the window or press \"Add files...\". Press Convert to start.";

/// Version line for the window title.
pub fn version_line() -> String {
    format!("rcsconv {} — .las/.laz/.pts → ReCap .rcs", env!("CARGO_PKG_VERSION"))
}

/// Pixels designed for 96 dpi -> physical pixels at `dpi`.
pub fn scale(px: i32, dpi: u32) -> i32 {
    let dpi = if dpi == 0 { 96 } else { dpi } as i32;
    (px * dpi + 48) / 96
}

/// Minimum client-area size at 96 dpi (width, height).
pub const MIN_SIZE_96: (i32, i32) = (900, 480);

/// Window layout: title, hint, button row, list.
///
/// `width`/`height` are the client area in physical pixels; the size is raised to
/// [`MIN_SIZE_96`] (the window refuses to shrink below it — `WM_GETMINMAXINFO`).
pub fn layout(width: i32, height: i32, dpi: u32) -> Layout {
    let margin = scale(12, dpi);
    let line = scale(20, dpi);
    let button_h = scale(28, dpi);
    let gap = scale(8, dpi);

    let width = width.max(scale(MIN_SIZE_96.0, dpi));
    let height = height.max(scale(MIN_SIZE_96.1, dpi));

    let title = Rect { x: margin, y: margin, w: width - margin * 2, h: line };
    let hint = Rect { x: margin, y: margin + line, w: width - margin * 2, h: line };

    let buttons_y = height - margin - button_h;
    let params_gap = scale(8, dpi);
    let progress_h = scale(16, dpi);
    let progress_y = buttons_y - gap - progress_h;
    let action_y = progress_y - gap - button_h;
    let row = scale(26, dpi);
    let row_gap = scale(6, dpi);
    let label_w = scale(112, dpi);
    let field_x = margin + label_w + gap / 2;
    let field_w = (width - margin - field_x).max(scale(80, dpi));

    // Four parameter rows from the bottom up: GUID, points, recenter, check boxes.
    let checks_y = action_y - params_gap - row;
    let row3_y = checks_y - row_gap - row;
    let row2_y = row3_y - row_gap - row;
    let row1_y = row2_y - row_gap - row;

    let browse_w = scale(90, dpi);
    let label_out_dir = Rect { x: margin, y: row1_y, w: label_w, h: row };
    let out_dir = Rect {
        x: field_x,
        y: row1_y,
        w: field_w - browse_w - gap,
        h: row,
    };
    let browse_out = Rect { x: width - margin - browse_w, y: row1_y, w: browse_w, h: row };

    let label_units = Rect { x: margin, y: row2_y, w: label_w, h: row };
    let units = Rect { x: field_x, y: row2_y, w: scale(70, dpi), h: row };
    let recenter_label_x = units.x + units.w + gap * 2;
    let label_recenter = Rect { x: recenter_label_x, y: row2_y, w: scale(100, dpi), h: row };
    let recenter = Rect {
        x: recenter_label_x + scale(100, dpi),
        y: row2_y,
        // The combo height is the height of the open list; it deliberately exceeds
        // the row (standard Win32 behaviour), so the rectangle is not part of
        // the bounds checks.
        w: scale(140, dpi),
        h: scale(160, dpi),
    };

    let label_max_tile = Rect { x: margin, y: row3_y, w: label_w, h: row };
    let max_tile = Rect { x: field_x, y: row3_y, w: scale(70, dpi), h: row };
    let guid_label_x = max_tile.x + max_tile.w + gap * 2;
    let label_guid = Rect { x: guid_label_x, y: row3_y, w: scale(50, dpi), h: row };
    let guid = Rect {
        x: guid_label_x + scale(50, dpi),
        y: row3_y,
        w: (width - margin - (guid_label_x + scale(50, dpi))).max(scale(80, dpi)),
        h: row,
    };

    let color = Rect { x: margin, y: checks_y, w: scale(90, dpi), h: row };
    let intensity =
        Rect { x: color.x + color.w + gap, y: checks_y, w: scale(150, dpi), h: row };
    let force = Rect {
        x: intensity.x + intensity.w + gap,
        y: checks_y,
        w: scale(200, dpi),
        h: row,
    };
    let recursive = Rect {
        x: force.x + force.w + gap,
        y: checks_y,
        w: (width - margin - (force.x + force.w + gap)).max(scale(140, dpi)),
        h: row,
    };

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

    let list_top = hint.y + hint.h + gap / 2;
    let list = Rect {
        x: margin,
        y: list_top,
        w: width - margin * 2,
        h: (row1_y - params_gap - list_top).max(scale(80, dpi)),
    };

    let progress = Rect { x: margin, y: progress_y, w: width - margin * 2, h: progress_h };
    let convert = Rect { x: margin, y: action_y, w: scale(170, dpi), h: button_h };
    let open_out = Rect {
        x: convert.x + convert.w + gap,
        y: action_y,
        w: scale(150, dpi),
        h: button_h,
    };

    Layout {
        title,
        hint,
        list,
        label_out_dir,
        out_dir,
        browse_out,
        label_units,
        units,
        label_recenter,
        recenter,
        label_max_tile,
        max_tile,
        label_guid,
        guid,
        color,
        intensity,
        force,
        recursive,
        convert,
        open_out,
        progress,
        add_files,
        add_folder,
        remove,
        clear,
        close,
    }
}

/// State of a queued file.
///
/// All variants except [`EntryStatus::Waiting`] appear during conversion; they are
/// kept here so the Status column is ready.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryStatus {
    /// Waiting for conversion.
    Waiting,
    /// Being converted right now.
    Running,
    /// The file was written.
    Written,
    /// Skipped (the file already exists).
    Skipped,
    /// Error.
    Failed,
    /// Cancelled by the user.
    Cancelled,
}

impl EntryStatus {
    /// Label for the Status column.
    pub fn label(self) -> &'static str {
        match self {
            EntryStatus::Waiting => "queued",
            EntryStatus::Running => "running...",
            EntryStatus::Written => "written",
            EntryStatus::Skipped => "skipped",
            EntryStatus::Failed => "failed",
            EntryStatus::Cancelled => "cancelled",
        }
    }
}

/// A queued file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// File path.
    pub path: PathBuf,
    /// Size in bytes (0 when it could not be read).
    pub size: u64,
    /// State.
    pub status: EntryStatus,
}

/// Queue of files to convert.
///
/// Duplicates are dropped by canonical path and directories are expanded into files
/// with the required extensions — the same [`rcs_conv::expand_sources`] the CLI
/// uses, so the GUI and the CLI see the same file set.
#[derive(Debug, Default, Clone)]
pub struct Queue {
    entries: Vec<Entry>,
}

impl Queue {
    /// Empty queue.
    pub fn new() -> Queue {
        Queue::default()
    }

    /// Number of files.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Files in the queue.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Paths of the queued files (needed to start the conversion).
    #[allow(dead_code)]
    pub fn paths(&self) -> Vec<PathBuf> {
        self.entries.iter().map(|entry| entry.path.clone()).collect()
    }

    /// Add files, directories and masks; return the number of new files.
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

    /// Remove a file by index; `true` when something was removed.
    pub fn remove(&mut self, index: usize) -> bool {
        if index < self.entries.len() {
            self.entries.remove(index);
            true
        } else {
            false
        }
    }

    /// Clear the queue.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Change the state of a file.
    #[allow(dead_code)]
    pub fn set_status(&mut self, index: usize, status: EntryStatus) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.status = status;
        }
    }

    /// Reset every file state to queued.
    #[allow(dead_code)]
    pub fn reset_statuses(&mut self) {
        for entry in &mut self.entries {
            entry.status = EntryStatus::Waiting;
        }
    }

    /// Total size of the files, bytes.
    pub fn total_size(&self) -> u64 {
        self.entries.iter().map(|entry| entry.size).sum()
    }
}

/// Duplicate key: the canonical path, or the original one.
fn canonical_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Size in bytes -> human-readable string (`1.20 MB`).
pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    if value < KB {
        format!("{bytes} B")
    } else if value < KB * KB {
        format!("{:.1} KB", value / KB)
    } else if value < KB * KB * KB {
        format!("{:.2} MB", value / (KB * KB))
    } else {
        format!("{:.2} GB", value / (KB * KB * KB))
    }
}

/// Hint line: how many files and their total size.
pub fn queue_hint(queue: &Queue) -> String {
    if queue.is_empty() {
        return HINT_EMPTY.to_string();
    }
    format!(
        "{} file(s) in the queue, {} total — press Convert to start.",
        queue.len(),
        format_size(queue.total_size())
    )
}

/// Parse the `GetOpenFileNameW` buffer with `OFN_ALLOWMULTISELECT`.
///
/// Win32 writes either one full path or "dir\0name1\0name2\0\0".
/// The function is pure (no Win32), so a plain test covers it.
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
        std::fs::create_dir_all(&dir).expect("temporary directory");
        dir
    }

    #[test]
    fn scale_keeps_96_dpi_as_is() {
        assert_eq!(scale(12, 96), 12);
        assert_eq!(scale(12, 0), 12, "0 dpi is treated as 96");
        assert_eq!(scale(12, 192), 24);
        assert_eq!(scale(12, 144), 18);
    }

    #[test]
    fn layout_stays_inside_client_area() {
        for (width, height, dpi) in [(900, 600, 96), (1400, 900, 144), (1320, 960, 192)] {
            let effective_width = width.max(scale(MIN_SIZE_96.0, dpi));
            let effective_height = height.max(scale(MIN_SIZE_96.1, dpi));
            let layout = layout(width, height, dpi);
            for rect in [
                layout.title,
                layout.hint,
                layout.list,
                layout.label_out_dir,
                layout.out_dir,
                layout.browse_out,
                layout.label_units,
                layout.units,
                layout.label_recenter,
                layout.label_max_tile,
                layout.max_tile,
                layout.label_guid,
                layout.guid,
                layout.color,
                layout.intensity,
                layout.force,
                layout.recursive,
                layout.add_files,
                layout.add_folder,
                layout.remove,
                layout.clear,
                layout.close,
            ] {
                assert!(rect.x >= 0 && rect.y >= 0, "{rect:?}");
                assert!(rect.x + rect.w <= effective_width, "{rect:?} at width {width}");
                assert!(rect.y + rect.h <= effective_height, "{rect:?} at height {height}");
            }
            assert!(layout.list.y > layout.hint.y, "the list is below the hint");
            assert!(layout.list.h > 0, "the list is not degenerate");
            assert!(layout.out_dir.y > layout.list.y, "parameters are below the list");
            assert!(layout.add_files.y > layout.recursive.y, "buttons are below the parameters");
            assert!(layout.close.x + layout.close.w <= effective_width, "Close is inside the window");
            assert!(layout.clear.x + layout.clear.w <= effective_width, "Clear is inside the window");
            assert!(layout.recenter.x + layout.recenter.w <= effective_width, "the combo box is inside the window");
        }
    }

    #[test]
    fn layout_survives_tiny_window() {
        // The window is below the minimum: the layout raises the size to MIN_SIZE_96.
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
            assert!(!ids[..index].contains(id), "id {id:?} repeats");
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
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.00 MB");
    }

    #[test]
    fn open_file_buffer_single_path() {
        let mut buffer: Vec<u16> = "D:\\scan\\one.laz".encode_utf16().collect();
        buffer.push(0);
        buffer.push(0);
        assert_eq!(parse_open_file_buffer(&buffer), vec![PathBuf::from("D:\\scan\\one.laz")]);
        assert!(parse_open_file_buffer(&[0, 0]).is_empty(), "an empty buffer means nothing");
    }

    #[test]
    fn open_file_buffer_multi_select() {
        // Directory, then file names, then an empty string.
        let mut buffer: Vec<u16> = "D:\\scan".encode_utf16().collect();
        for name in ["a.laz", "b.las"] {
            buffer.push(0);
            buffer.extend(name.encode_utf16());
        }
        buffer.push(0);
        buffer.push(0);
        assert_eq!(
            parse_open_file_buffer(&buffer),
            vec![PathBuf::from("D:\\scan").join("a.laz"), PathBuf::from("D:\\scan").join("b.las")]
        );
    }

    #[test]
    fn queue_adds_files_directories_and_skips_duplicates() {
        let dir = temp_dir("queue");
        std::fs::create_dir_all(dir.join("sub")).expect("subdirectory");
        for name in ["a.laz", "b.LAS", "c.pts", "notes.txt"] {
            std::fs::write(dir.join(name), b"0123456789").expect("file");
        }
        std::fs::write(dir.join("sub").join("d.laz"), b"01234").expect("file");

        let mut queue = Queue::new();
        assert_eq!(queue.add(&[dir.clone()], false), 3, "no recursion: directory files");
        assert_eq!(queue.len(), 3);

        assert_eq!(queue.add(&[dir.clone()], false), 0, "duplicates are not added");
        assert_eq!(queue.len(), 3);

        assert_eq!(queue.add(&[dir.clone()], true), 1, "with subdirectory recursion");
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
        std::fs::write(&file, b"x").expect("file");

        let mut queue = Queue::new();
        queue.add(&[file], false);
        queue.set_status(0, EntryStatus::Written);
        assert_eq!(queue.entries()[0].status, EntryStatus::Written);
        queue.reset_statuses();
        assert_eq!(queue.entries()[0].status, EntryStatus::Waiting);
        assert_eq!(EntryStatus::Waiting.label(), "queued");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
