//! Thin wrappers over Win32 — the only place in the GUI that needs `unsafe`.
//!
//! Project rules (`.clinerules` 10): `unsafe` is localised, every call is
//! accompanied by a `SAFETY:` comment, and the other modules work through these
//! functions without seeing `unsafe`.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, RECT, ERROR_SUCCESS};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows_sys::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OPENFILENAMEW, OFN_ALLOWMULTISELECT, OFN_EXPLORER, OFN_FILEMUSTEXIST,
    OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
};
use windows_sys::Win32::UI::Controls::{
    InitCommonControlsEx, BST_CHECKED, ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES,
    INITCOMMONCONTROLSEX, LVCF_SUBITEM, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT, LVITEMW,
    LVM_DELETEALLITEMS, LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTITEMW,
    LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMTEXTW, LVS_EX_FULLROWSELECT, LVS_EX_GRIDLINES,
    LVNI_SELECTED, PBM_SETPOS, PBM_SETRANGE32,
};
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForSystem, GetDpiForWindow, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Shell::{
    DragAcceptFiles, DragFinish, DragQueryFileW, SHBrowseForFolderW, SHGetPathFromIDListW,
    ShellExecuteW, BIF_EDITBOX, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetClientRect, GetDlgItem, GetWindowTextLengthW, GetWindowTextW,
    MoveWindow, SendMessageW, SetWindowTextW, ShowWindow, BM_GETCHECK, BM_SETCHECK, CB_ADDSTRING,
    CB_GETCURSEL, CB_SETCURSEL, SW_SHOW, SW_SHOWNORMAL,
};

use crate::model::{Rect, COLUMNS};

/// NUL-terminated UTF-16 string (for Win32 `*W` functions).
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `HINSTANCE` of the current process (`HMODULE` and `HINSTANCE` are the same pointer).
pub fn instance() -> HINSTANCE {
    // SAFETY: GetModuleHandleW(NULL) is the documented way to get the process HMODULE.
    unsafe { GetModuleHandleW(ptr::null()) }
}

/// Enable `PerMonitorV2` DPI awareness.
///
/// The manifest does the same; this call is a fallback for running without it
/// (and the only way if `windres` was unavailable during the build).
pub fn enable_dpi_awareness() {
    // SAFETY: the function takes no pointers and needs no resource release.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// dpi of the primary monitor (for the initial window size).
pub fn system_dpi() -> u32 {
    // SAFETY: no arguments.
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 {
        96
    } else {
        dpi
    }
}

/// dpi of the window (may differ from the system one after moving to another monitor).
pub fn dpi_of(hwnd: HWND) -> u32 {
    // SAFETY: hwnd comes from Win32 and lives until it is destroyed.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        system_dpi()
    } else {
        dpi
    }
}

/// Client area size of the window.
pub fn client_size(hwnd: HWND) -> (i32, i32) {
    // SAFETY: RECT is a plain struct; GetClientRect fills it for a valid hwnd.
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetClientRect(hwnd, &mut rect) };
    if ok == 0 {
        (0, 0)
    } else {
        (rect.right - rect.left, rect.bottom - rect.top)
    }
}

/// Create a child window (`STATIC`, `BUTTON`, ...) with the given `id`.
pub fn create_child(parent: HWND, class: &str, text: &str, style: u32, id: usize) -> HWND {
    create_child_ex(parent, 0, class, text, style, id)
}

/// Same, but with an extended style (`WS_EX_CLIENTEDGE` for the list).
pub fn create_child_ex(
    parent: HWND,
    ex_style: u32,
    class: &str,
    text: &str,
    style: u32,
    id: usize,
) -> HWND {
    let class = wide(class);
    let text = wide(text);
    // SAFETY: the strings live to the end of the expression; parent is valid, id is our control.
    unsafe {
        CreateWindowExW(
            ex_style,
            class.as_ptr(),
            text.as_ptr(),
            style,
            0,
            0,
            0,
            0,
            parent,
            id as isize as *mut c_void,
            instance(),
            ptr::null(),
        )
    }
}

/// Move or resize the child window with the given `id`.
pub fn move_child(parent: HWND, id: usize, rect: Rect) {
    // SAFETY: GetDlgItem returns NULL when the control is missing — checked before MoveWindow.
    let child = unsafe { GetDlgItem(parent, id as i32) };
    if child.is_null() {
        return;
    }
    // SAFETY: child is found in parent; coordinates are plain i32.
    unsafe {
        MoveWindow(child, rect.x, rect.y, rect.w, rect.h, 1);
    }
}

/// Show the window.
pub fn show(hwnd: HWND) {
    // SAFETY: hwnd is created and not destroyed yet.
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
    }
}

/// Destroy the window (the `WM_DESTROY` message then ends the loop).
pub fn destroy(hwnd: HWND) {
    // SAFETY: called from WM_COMMAND/WM_CLOSE for a live window.
    unsafe {
        DestroyWindow(hwnd);
    }
}

/// Register the standard control classes (list, buttons).
pub fn init_common_controls() {
    let mut settings = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_LISTVIEW_CLASSES | ICC_STANDARD_CLASSES,
    };
    // SAFETY: the struct is fully initialised and the size is passed correctly.
    unsafe {
        InitCommonControlsEx(&mut settings);
    }
}

/// Child window by `id` (`HWND::default()` when the control is missing).
pub fn child(parent: HWND, id: usize) -> HWND {
    // SAFETY: parent is a live window; NULL is returned when the control is missing.
    unsafe { GetDlgItem(parent, id as i32) }
}

/// Replace the control text.
pub fn set_text(hwnd: HWND, text: &str) {
    if hwnd.is_null() {
        return;
    }
    let value = wide(text);
    // SAFETY: the string lives to the end of the call; hwnd is checked for NULL.
    unsafe {
        SetWindowTextW(hwnd, value.as_ptr());
    }
}

/// Accept dropped files (`WM_DROPFILES`).
pub fn accept_dropped_files(hwnd: HWND) {
    // SAFETY: hwnd is a live window.
    unsafe {
        DragAcceptFiles(hwnd, 1);
    }
}

/// Files from the `WM_DROPFILES` message (the list is freed with `DragFinish`).
pub fn dropped_files(hdrop: *mut c_void) -> Vec<PathBuf> {
    // SAFETY: hdrop is valid while WM_DROPFILES is handled.
    let count = unsafe { DragQueryFileW(hdrop, u32::MAX, ptr::null_mut(), 0) };
    let mut files = Vec::with_capacity(count as usize);
    for index in 0..count {
        // SAFETY: same hdrop; NULL with zero length means a size query.
        let length = unsafe { DragQueryFileW(hdrop, index, ptr::null_mut(), 0) };
        if length == 0 {
            continue;
        }
        let mut buffer = vec![0u16; length as usize + 1];
        // SAFETY: the buffer is large enough, hdrop is valid.
        let written =
            unsafe { DragQueryFileW(hdrop, index, buffer.as_mut_ptr(), buffer.len() as u32) };
        if written > 0 {
            files.push(PathBuf::from(String::from_utf16_lossy(&buffer[..written as usize])));
        }
    }
    // SAFETY: hdrop must be freed exactly once after reading.
    unsafe {
        DragFinish(hdrop);
    }
    files
}

/// File selection dialog (`GetOpenFileNameW`, multi-select).
///
/// `filter` holds "description, mask" pairs (`"Point clouds", "*.las;*.laz"`).
/// An empty vector means the dialog was cancelled.
pub fn open_files(owner: HWND, title: &str, filter: &[(&str, &str)]) -> Vec<PathBuf> {
    let mut filter_text = String::new();
    for (label, mask) in filter {
        filter_text.push_str(label);
        filter_text.push('\0');
        filter_text.push_str(mask);
        filter_text.push('\0');
    }
    filter_text.push('\0');
    let filter_w: Vec<u16> = filter_text.encode_utf16().collect();
    let title_w = wide(title);

    // Buffer for the file list: in multi-select mode Win32 writes the directory then names.
    let mut buffer = vec![0u16; 64 * 1024];
    // SAFETY: OPENFILENAMEW is a plain struct, zeros are a valid initial value.
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.hwndOwner = owner;
    dialog.lpstrFilter = filter_w.as_ptr();
    dialog.nFilterIndex = 1;
    dialog.lpstrFile = buffer.as_mut_ptr();
    dialog.nMaxFile = buffer.len() as u32;
    dialog.lpstrTitle = title_w.as_ptr();
    dialog.Flags = OFN_ALLOWMULTISELECT
        | OFN_EXPLORER
        | OFN_FILEMUSTEXIST
        | OFN_PATHMUSTEXIST
        | OFN_HIDEREADONLY
        | OFN_NOCHANGEDIR;

    // SAFETY: all pointers in dialog are valid and live to the end of the call.
    let accepted = unsafe { GetOpenFileNameW(&mut dialog) };
    if accepted == 0 {
        return Vec::new();
    }
    crate::model::parse_open_file_buffer(&buffer)
}

/// Directory selection dialog (`SHBrowseForFolderW`).
pub fn browse_folder(owner: HWND, caption: &str) -> Option<PathBuf> {
    let caption_w = wide(caption);
    let mut display = vec![0u16; 260];
    // SAFETY: BROWSEINFOW is a plain struct; zeros are a valid initial value.
    let mut info: BROWSEINFOW = unsafe { std::mem::zeroed() };
    info.hwndOwner = owner;
    info.pszDisplayName = display.as_mut_ptr();
    info.lpszTitle = caption_w.as_ptr();
    info.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX;

    // SAFETY: the struct is filled; the returned PIDL is allocated by the shell.
    let pidl = unsafe { SHBrowseForFolderW(&info) };
    if pidl.is_null() {
        return None;
    }
    let mut path = vec![0u16; 32 * 1024];
    // SAFETY: pidl is valid, the buffer is large enough.
    let ok = unsafe { SHGetPathFromIDListW(pidl, path.as_mut_ptr()) };
    // SAFETY: the PIDL is allocated by the shell via CoTaskMemAlloc — freed exactly once.
    unsafe {
        CoTaskMemFree(pidl as *const c_void);
    }
    if ok == 0 {
        return None;
    }
    let length = path.iter().position(|&value| value == 0).unwrap_or(0);
    Some(PathBuf::from(String::from_utf16_lossy(&path[..length])))
}

/// Create the list columns (widths from [`crate::model::COLUMNS`], scaled by dpi).
pub fn list_init(list: HWND, dpi: u32) {
    if list.is_null() {
        return;
    }
    let extended = LVS_EX_FULLROWSELECT | LVS_EX_GRIDLINES;
    // SAFETY: list is a live control; the flags are plain u32.
    unsafe {
        SendMessageW(list, LVM_SETEXTENDEDLISTVIEWSTYLE, extended as usize, extended as isize);
    }
    for (index, (title, width)) in COLUMNS.iter().enumerate() {
        let text = wide(title);
        // SAFETY: LVCOLUMNW is a plain struct; zeros are valid, the fields are set below.
        let mut column: LVCOLUMNW = unsafe { std::mem::zeroed() };
        column.mask = LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM;
        column.pszText = text.as_ptr() as *mut u16;
        column.cchTextMax = text.len() as i32;
        column.cx = crate::model::scale(*width, dpi);
        column.iSubItem = index as i32;
        // SAFETY: Win32 copies the string and the struct during the call.
        unsafe {
            SendMessageW(list, LVM_INSERTCOLUMNW, index, &column as *const _ as isize);
        }
    }
}

/// Clear the file list.
pub fn list_clear(list: HWND) {
    // SAFETY: list is a live control.
    unsafe {
        SendMessageW(list, LVM_DELETEALLITEMS, 0, 0);
    }
}

/// Add a row: `cells[0]` is the first column, the rest are subitems.
pub fn list_add_row(list: HWND, row: usize, cells: &[String]) {
    let Some(first_cell) = cells.first() else {
        return;
    };
    let first = wide(first_cell);
    // SAFETY: LVITEMW is a plain struct; zeros are valid, the fields are set below.
    let mut item: LVITEMW = unsafe { std::mem::zeroed() };
    item.mask = LVIF_TEXT;
    item.iItem = row as i32;
    item.iSubItem = 0;
    item.pszText = first.as_ptr() as *mut u16;
    item.cchTextMax = first.len() as i32;
    // SAFETY: Win32 copies the string and the struct during the call.
    unsafe {
        SendMessageW(list, LVM_INSERTITEMW, 0, &item as *const _ as isize);
    }

    // Subitem strings live to the end of the function — Win32 copies them itself.
    let rest: Vec<Vec<u16>> = cells[1..].iter().map(|cell| wide(cell)).collect();
    for (offset, text) in rest.iter().enumerate() {
        // SAFETY: same struct as above; the memory is zeroed.
        let mut sub: LVITEMW = unsafe { std::mem::zeroed() };
        sub.iSubItem = (offset + 1) as i32;
        sub.pszText = text.as_ptr() as *mut u16;
        // SAFETY: Win32 copies the string during the call.
        unsafe {
            SendMessageW(list, LVM_SETITEMTEXTW, row, &sub as *const _ as isize);
        }
    }
}

/// Index of the selected list row.
pub fn list_selected(list: HWND) -> Option<usize> {
    // SAFETY: LVM_GETNEXTITEM with wParam = -1 searches from the start; returns -1 when empty.
    let index = unsafe { SendMessageW(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize) };
    if index < 0 {
        None
    } else {
        Some(index as usize)
    }
}

/// Control text (`Edit`, `Static`, `ComboBox`).
pub fn text_of(hwnd: HWND) -> String {
    if hwnd.is_null() {
        return String::new();
    }
    // SAFETY: hwnd is a live control.
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    // SAFETY: the buffer is large enough (length + NUL).
    let written = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    if written <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..written as usize])
}

/// Fill the combo box and select an item.
pub fn combo_fill(combo: HWND, items: &[&str], selected: usize) {
    if combo.is_null() {
        return;
    }
    for item in items {
        let text = wide(item);
        // SAFETY: Win32 copies the string during the call.
        unsafe {
            SendMessageW(combo, CB_ADDSTRING, 0, text.as_ptr() as isize);
        }
    }
    combo_select(combo, selected.min(items.len().saturating_sub(1)));
}

/// Select a combo box item.
pub fn combo_select(combo: HWND, index: usize) {
    if combo.is_null() {
        return;
    }
    // SAFETY: the index stays within the added items (the caller checks it).
    unsafe {
        SendMessageW(combo, CB_SETCURSEL, index, 0);
    }
}

/// Selected combo box item.
pub fn combo_selected(combo: HWND) -> Option<usize> {
    if combo.is_null() {
        return None;
    }
    // SAFETY: CB_GETCURSEL returns -1 when nothing is selected.
    let index = unsafe { SendMessageW(combo, CB_GETCURSEL, 0, 0) };
    if index < 0 {
        None
    } else {
        Some(index as usize)
    }
}

/// Set or clear a check box.
pub fn check_set(control: HWND, checked: bool) {
    if control.is_null() {
        return;
    }
    let state = if checked { BST_CHECKED as usize } else { 0 };
    // SAFETY: control is a check box (BS_AUTOCHECKBOX), the state is a plain number.
    unsafe {
        SendMessageW(control, BM_SETCHECK, state, 0);
    }
}

/// Whether the check box is checked.
pub fn check_get(control: HWND) -> bool {
    if control.is_null() {
        return false;
    }
    // SAFETY: control is a check box; BM_GETCHECK writes no memory.
    let state = unsafe { SendMessageW(control, BM_GETCHECK, 0, 0) };
    state == BST_CHECKED as isize
}

/// Registry key holding the window settings.
const SETTINGS_KEY: &str = "Software\\rcsconv\\gui";

/// Open (create) the settings key under `HKCU`.
fn open_settings() -> Option<HKEY> {
    let subkey = wide(SETTINGS_KEY);
    let mut key: HKEY = ptr::null_mut();
    // SAFETY: subkey lives to the end of the call; no security attributes are needed (NULL).
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_READ | KEY_WRITE,
            ptr::null(),
            &mut key,
            ptr::null_mut(),
        )
    };
    if status == ERROR_SUCCESS {
        Some(key)
    } else {
        None
    }
}

/// Save the settings to `HKCU\Software\rcsconv\gui` (errors are silently ignored).
pub fn registry_save(values: &[(String, String)]) {
    let Some(key) = open_settings() else {
        return;
    };
    for (name, value) in values {
        let name_w = wide(name);
        let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = std::mem::size_of_val(data.as_slice()) as u32;
        // SAFETY: the name and data live to the end of the call; type is REG_SZ, size in bytes.
        unsafe {
            RegSetValueExW(key, name_w.as_ptr(), 0, REG_SZ, data.as_ptr() as *const u8, bytes);
        }
    }
    // SAFETY: the key comes from RegCreateKeyExW and is released exactly once.
    unsafe {
        RegCloseKey(key);
    }
}

/// Read the string values from `HKCU\Software\rcsconv\gui`.
pub fn registry_load(names: &[&str]) -> Vec<(String, String)> {
    let Some(key) = open_settings() else {
        return Vec::new();
    };
    let mut values = Vec::new();
    for name in names {
        let name_w = wide(name);
        let mut kind: u32 = 0;
        let mut size: u32 = 0;
        // SAFETY: the first call only queries the data size.
        let status = unsafe {
            RegQueryValueExW(
                key,
                name_w.as_ptr(),
                ptr::null(),
                &mut kind,
                ptr::null_mut(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS || size == 0 || size > 64 * 1024 {
            continue;
        }
        let mut buffer = vec![0u16; size as usize / 2 + 2];
        let mut read = std::mem::size_of_val(buffer.as_slice()) as u32;
        // SAFETY: the buffer is large enough and the size is passed in bytes.
        let status = unsafe {
            RegQueryValueExW(
                key,
                name_w.as_ptr(),
                ptr::null(),
                &mut kind,
                buffer.as_mut_ptr() as *mut u8,
                &mut read,
            )
        };
        if status != ERROR_SUCCESS {
            continue;
        }
        let length = (read as usize / 2).min(buffer.len());
        let mut text = String::from_utf16_lossy(&buffer[..length]);
        if let Some(position) = text.find('\0') {
            text.truncate(position);
        }
        values.push((name.to_string(), text));
    }
    // SAFETY: the key is released exactly once.
    unsafe {
        RegCloseKey(key);
    }
    values
}

/// Set the progress bar range (`0..=max`).
pub fn progress_range(bar: HWND, max: u32) {
    if bar.is_null() {
        return;
    }
    // SAFETY: bar is a live progress bar; the range is plain numbers.
    unsafe {
        SendMessageW(bar, PBM_SETRANGE32, 0, max as isize);
    }
}

/// Set the progress bar position.
pub fn progress_set(bar: HWND, value: u32) {
    if bar.is_null() {
        return;
    }
    // SAFETY: bar is a live progress bar.
    unsafe {
        SendMessageW(bar, PBM_SETPOS, value as usize, 0);
    }
}

/// Open a path in Explorer (or the associated program).
pub fn shell_open(path: &Path) -> bool {
    let operation = wide("open");
    let target = wide(&path.display().to_string());
    // SAFETY: the strings live to the end of the call; the file is opened by association.
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW returns a value > 32 on success (as the documentation requires).
    result as isize > 32
}

/// Update one list cell (`row` is the row, `column` the column).
pub fn list_set_cell(list: HWND, row: usize, column: usize, text: &str) {
    if list.is_null() {
        return;
    }
    let value = wide(text);
    // SAFETY: LVITEMW is a plain struct; zeros are valid, the fields are set below.
    let mut item: LVITEMW = unsafe { std::mem::zeroed() };
    item.iSubItem = column as i32;
    item.pszText = value.as_ptr() as *mut u16;
    // SAFETY: Win32 copies the string during the call.
    unsafe {
        SendMessageW(list, LVM_SETITEMTEXTW, row, &item as *const _ as isize);
    }
}
