//! Тонкие обёртки над Win32 — единственное место в GUI, где нужен `unsafe`.
//!
//! Правила проекта (`.clinerules` п.10): `unsafe` локализован, каждый вызов
//! сопровождается комментарием `SAFETY:`, остальные модули работают через эти
//! функции и `unsafe` не видят.

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, RECT};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OPENFILENAMEW, OFN_ALLOWMULTISELECT, OFN_EXPLORER, OFN_FILEMUSTEXIST,
    OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
};
use windows_sys::Win32::UI::Controls::{
    InitCommonControlsEx, ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX,
    LVCF_SUBITEM, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT, LVITEMW, LVM_DELETEALLITEMS,
    LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETEXTENDEDLISTVIEWSTYLE,
    LVM_SETITEMTEXTW, LVS_EX_FULLROWSELECT, LVS_EX_GRIDLINES, LVNI_SELECTED,
};
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForSystem, GetDpiForWindow, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Shell::{
    DragAcceptFiles, DragFinish, DragQueryFileW, SHBrowseForFolderW, SHGetPathFromIDListW,
    BIF_EDITBOX, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetClientRect, GetDlgItem, MoveWindow, SendMessageW,
    SetWindowTextW, ShowWindow, SW_SHOW,
};

use crate::model::{Rect, COLUMNS};

/// UTF-16-строка с завершающим нулём (для `*W`-функций Win32).
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `HINSTANCE` текущего процесса (`HMODULE` и `HINSTANCE` — один и тот же указатель).
pub fn instance() -> HINSTANCE {
    // SAFETY: GetModuleHandleW(NULL) — документированный способ получить HMODULE процесса.
    unsafe { GetModuleHandleW(ptr::null()) }
}

/// Включить DPI-awareness `PerMonitorV2`.
///
/// То же самое делает манифест; вызов — страховка на случай запуска без него
/// (а также единственный способ, если `windres` был недоступен при сборке).
pub fn enable_dpi_awareness() {
    // SAFETY: функция не принимает указателей и не требует освобождения ресурсов.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// dpi основного монитора (для первичного размера окна).
pub fn system_dpi() -> u32 {
    // SAFETY: аргументов нет.
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 {
        96
    } else {
        dpi
    }
}

/// dpi окна (может отличаться от системного при переносе на другой монитор).
pub fn dpi_of(hwnd: HWND) -> u32 {
    // SAFETY: hwnd получен от Win32 и живёт, пока не уничтожен.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        system_dpi()
    } else {
        dpi
    }
}

/// Размер клиентской области окна.
pub fn client_size(hwnd: HWND) -> (i32, i32) {
    // SAFETY: RECT — обычная структура; GetClientRect заполняет её при валидном hwnd.
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetClientRect(hwnd, &mut rect) };
    if ok == 0 {
        (0, 0)
    } else {
        (rect.right - rect.left, rect.bottom - rect.top)
    }
}

/// Создать дочернее окно (`STATIC`, `BUTTON`, …) с заданным `id`.
pub fn create_child(parent: HWND, class: &str, text: &str, style: u32, id: usize) -> HWND {
    create_child_ex(parent, 0, class, text, style, id)
}

/// То же, но с расширенным стилем (`WS_EX_CLIENTEDGE` для списка).
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
    // SAFETY: строки живут до конца выражения; parent валиден, id — наш контрол.
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

/// Переместить/растянуть дочернее окно по `id`.
pub fn move_child(parent: HWND, id: usize, rect: Rect) {
    // SAFETY: GetDlgItem вернёт NULL, если контрола нет — проверяем до MoveWindow.
    let child = unsafe { GetDlgItem(parent, id as i32) };
    if child.is_null() {
        return;
    }
    // SAFETY: child найден в parent; координаты — обычные i32.
    unsafe {
        MoveWindow(child, rect.x, rect.y, rect.w, rect.h, 1);
    }
}

/// Показать окно.
pub fn show(hwnd: HWND) {
    // SAFETY: hwnd создан и ещё не уничтожен.
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
    }
}

/// Уничтожить окно (после этого сообщение `WM_DESTROY` завершит цикл).
pub fn destroy(hwnd: HWND) {
    // SAFETY: вызывается по WM_COMMAND/WM_CLOSE для живого окна.
    unsafe {
        DestroyWindow(hwnd);
    }
}

/// Зарегистрировать классы стандартных контролов (список, кнопки).
pub fn init_common_controls() {
    let mut settings = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_LISTVIEW_CLASSES | ICC_STANDARD_CLASSES,
    };
    // SAFETY: структура инициализирована полностью, размер передан верно.
    unsafe {
        InitCommonControlsEx(&mut settings);
    }
}

/// Дочернее окно по `id` (`HWND::default()`, если контрола нет).
pub fn child(parent: HWND, id: usize) -> HWND {
    // SAFETY: parent — живое окно; при отсутствии контрола вернётся NULL.
    unsafe { GetDlgItem(parent, id as i32) }
}

/// Заменить текст контрола.
pub fn set_text(hwnd: HWND, text: &str) {
    if hwnd.is_null() {
        return;
    }
    let value = wide(text);
    // SAFETY: строка живёт до конца вызова; hwnd проверен на NULL.
    unsafe {
        SetWindowTextW(hwnd, value.as_ptr());
    }
}

/// Разрешить приём перетащенных файлов (`WM_DROPFILES`).
pub fn accept_dropped_files(hwnd: HWND) {
    // SAFETY: hwnd — живое окно.
    unsafe {
        DragAcceptFiles(hwnd, 1);
    }
}

/// Файлы из сообщения `WM_DROPFILES` (список освобождается через `DragFinish`).
pub fn dropped_files(hdrop: *mut c_void) -> Vec<PathBuf> {
    // SAFETY: hdrop валиден в пределах обработки WM_DROPFILES.
    let count = unsafe { DragQueryFileW(hdrop, u32::MAX, ptr::null_mut(), 0) };
    let mut files = Vec::with_capacity(count as usize);
    for index in 0..count {
        // SAFETY: тот же hdrop; NULL и нулевая длина — запрос размера.
        let length = unsafe { DragQueryFileW(hdrop, index, ptr::null_mut(), 0) };
        if length == 0 {
            continue;
        }
        let mut buffer = vec![0u16; length as usize + 1];
        // SAFETY: буфер достаточного размера, hdrop валиден.
        let written =
            unsafe { DragQueryFileW(hdrop, index, buffer.as_mut_ptr(), buffer.len() as u32) };
        if written > 0 {
            files.push(PathBuf::from(String::from_utf16_lossy(&buffer[..written as usize])));
        }
    }
    // SAFETY: освобождать hdrop нужно ровно один раз после чтения.
    unsafe {
        DragFinish(hdrop);
    }
    files
}

/// Диалог выбора файлов (`GetOpenFileNameW`, мультивыбор).
///
/// `filter` — пары «описание, маска» (`"Облака точек", "*.las;*.laz"`).
/// Пустой вектор означает отмену.
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

    // Буфер под список файлов: при мультивыборе Win32 пишет каталог и имена.
    let mut buffer = vec![0u16; 64 * 1024];
    // SAFETY: OPENFILENAMEW — обычная структура, нули — корректное начальное значение.
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

    // SAFETY: все указатели в dialog валидны и живут до конца вызова.
    let accepted = unsafe { GetOpenFileNameW(&mut dialog) };
    if accepted == 0 {
        return Vec::new();
    }
    crate::model::parse_open_file_buffer(&buffer)
}

/// Диалог выбора каталога (`SHBrowseForFolderW`).
pub fn browse_folder(owner: HWND, caption: &str) -> Option<PathBuf> {
    let caption_w = wide(caption);
    let mut display = vec![0u16; 260];
    // SAFETY: BROWSEINFOW — обычная структура; нули — корректное начальное значение.
    let mut info: BROWSEINFOW = unsafe { std::mem::zeroed() };
    info.hwndOwner = owner;
    info.pszDisplayName = display.as_mut_ptr();
    info.lpszTitle = caption_w.as_ptr();
    info.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX;

    // SAFETY: структура заполнена; возвращённый PIDL выделен оболочкой.
    let pidl = unsafe { SHBrowseForFolderW(&info) };
    if pidl.is_null() {
        return None;
    }
    let mut path = vec![0u16; 32 * 1024];
    // SAFETY: pidl валиден, буфер достаточного размера.
    let ok = unsafe { SHGetPathFromIDListW(pidl, path.as_mut_ptr()) };
    // SAFETY: PIDL выделен оболочкой через CoTaskMemAlloc — освобождаем один раз.
    unsafe {
        CoTaskMemFree(pidl as *const c_void);
    }
    if ok == 0 {
        return None;
    }
    let length = path.iter().position(|&value| value == 0).unwrap_or(0);
    Some(PathBuf::from(String::from_utf16_lossy(&path[..length])))
}

/// Создать колонки списка (ширины из [`crate::model::COLUMNS`], масштаб — по dpi).
pub fn list_init(list: HWND, dpi: u32) {
    if list.is_null() {
        return;
    }
    let extended = LVS_EX_FULLROWSELECT | LVS_EX_GRIDLINES;
    // SAFETY: list — живой контрол; флаги — обычные u32.
    unsafe {
        SendMessageW(list, LVM_SETEXTENDEDLISTVIEWSTYLE, extended as usize, extended as isize);
    }
    for (index, (title, width)) in COLUMNS.iter().enumerate() {
        let text = wide(title);
        // SAFETY: LVCOLUMNW — обычная структура; нули корректны, поля задаются ниже.
        let mut column: LVCOLUMNW = unsafe { std::mem::zeroed() };
        column.mask = LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM;
        column.pszText = text.as_ptr() as *mut u16;
        column.cchTextMax = text.len() as i32;
        column.cx = crate::model::scale(*width, dpi);
        column.iSubItem = index as i32;
        // SAFETY: Win32 копирует строку и структуру во время вызова.
        unsafe {
            SendMessageW(list, LVM_INSERTCOLUMNW, index, &column as *const _ as isize);
        }
    }
}

/// Очистить список файлов.
pub fn list_clear(list: HWND) {
    // SAFETY: list — живой контрол.
    unsafe {
        SendMessageW(list, LVM_DELETEALLITEMS, 0, 0);
    }
}

/// Добавить строку: `cells[0]` — первый столбец, остальные — подэлементы.
pub fn list_add_row(list: HWND, row: usize, cells: &[String]) {
    let Some(first_cell) = cells.first() else {
        return;
    };
    let first = wide(first_cell);
    // SAFETY: LVITEMW — обычная структура; нули корректны, поля задаются ниже.
    let mut item: LVITEMW = unsafe { std::mem::zeroed() };
    item.mask = LVIF_TEXT;
    item.iItem = row as i32;
    item.iSubItem = 0;
    item.pszText = first.as_ptr() as *mut u16;
    item.cchTextMax = first.len() as i32;
    // SAFETY: Win32 копирует строку и структуру во время вызова.
    unsafe {
        SendMessageW(list, LVM_INSERTITEMW, 0, &item as *const _ as isize);
    }

    // Строки для подэлементов живут до конца функции — Win32 копирует их сам.
    let rest: Vec<Vec<u16>> = cells[1..].iter().map(|cell| wide(cell)).collect();
    for (offset, text) in rest.iter().enumerate() {
        // SAFETY: та же структура, что выше; память нулевая.
        let mut sub: LVITEMW = unsafe { std::mem::zeroed() };
        sub.iSubItem = (offset + 1) as i32;
        sub.pszText = text.as_ptr() as *mut u16;
        // SAFETY: Win32 копирует строку во время вызова.
        unsafe {
            SendMessageW(list, LVM_SETITEMTEXTW, row, &sub as *const _ as isize);
        }
    }
}

/// Индекс выделенной строки списка.
pub fn list_selected(list: HWND) -> Option<usize> {
    // SAFETY: LVM_GETNEXTITEM с wParam = -1 ищет с начала; вернёт -1, если пусто.
    let index = unsafe { SendMessageW(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize) };
    if index < 0 {
        None
    } else {
        Some(index as usize)
    }
}
