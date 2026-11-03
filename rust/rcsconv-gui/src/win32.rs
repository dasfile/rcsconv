//! Тонкие обёртки над Win32 — единственное место в GUI, где нужен `unsafe`.
//!
//! Правила проекта (`.clinerules` п.10): `unsafe` локализован, каждый вызов
//! сопровождается комментарием `SAFETY:`, остальные модули работают через эти
//! функции и `unsafe` не видят.

use std::ffi::c_void;
use std::ptr;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, RECT};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForSystem, GetDpiForWindow, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetClientRect, GetDlgItem, MoveWindow, ShowWindow, SW_SHOW,
};

use crate::model::Rect;

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
    let class = wide(class);
    let text = wide(text);
    // SAFETY: строки живут до конца выражения; parent валиден, id — наш контрол.
    unsafe {
        CreateWindowExW(
            0,
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
