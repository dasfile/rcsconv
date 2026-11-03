//! РўРѕРЅРєРёРµ РѕР±С‘СЂС‚РєРё РЅР°Рґ Win32 вЂ” РµРґРёРЅСЃС‚РІРµРЅРЅРѕРµ РјРµСЃС‚Рѕ РІ GUI, РіРґРµ РЅСѓР¶РµРЅ `unsafe`.
//!
//! РџСЂР°РІРёР»Р° РїСЂРѕРµРєС‚Р° (`.clinerules` Рї.10): `unsafe` Р»РѕРєР°Р»РёР·РѕРІР°РЅ, РєР°Р¶РґС‹Р№ РІС‹Р·РѕРІ
//! СЃРѕРїСЂРѕРІРѕР¶РґР°РµС‚СЃСЏ РєРѕРјРјРµРЅС‚Р°СЂРёРµРј `SAFETY:`, РѕСЃС‚Р°Р»СЊРЅС‹Рµ РјРѕРґСѓР»Рё СЂР°Р±РѕС‚Р°СЋС‚ С‡РµСЂРµР· СЌС‚Рё
//! С„СѓРЅРєС†РёРё Рё `unsafe` РЅРµ РІРёРґСЏС‚.

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

/// UTF-16-СЃС‚СЂРѕРєР° СЃ Р·Р°РІРµСЂС€Р°СЋС‰РёРј РЅСѓР»С‘Рј (РґР»СЏ `*W`-С„СѓРЅРєС†РёР№ Win32).
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `HINSTANCE` С‚РµРєСѓС‰РµРіРѕ РїСЂРѕС†РµСЃСЃР° (`HMODULE` Рё `HINSTANCE` вЂ” РѕРґРёРЅ Рё С‚РѕС‚ Р¶Рµ СѓРєР°Р·Р°С‚РµР»СЊ).
pub fn instance() -> HINSTANCE {
    // SAFETY: GetModuleHandleW(NULL) вЂ” РґРѕРєСѓРјРµРЅС‚РёСЂРѕРІР°РЅРЅС‹Р№ СЃРїРѕСЃРѕР± РїРѕР»СѓС‡РёС‚СЊ HMODULE РїСЂРѕС†РµСЃСЃР°.
    unsafe { GetModuleHandleW(ptr::null()) }
}

/// Р’РєР»СЋС‡РёС‚СЊ DPI-awareness `PerMonitorV2`.
///
/// РўРѕ Р¶Рµ СЃР°РјРѕРµ РґРµР»Р°РµС‚ РјР°РЅРёС„РµСЃС‚; РІС‹Р·РѕРІ вЂ” СЃС‚СЂР°С…РѕРІРєР° РЅР° СЃР»СѓС‡Р°Р№ Р·Р°РїСѓСЃРєР° Р±РµР· РЅРµРіРѕ
/// (Р° С‚Р°РєР¶Рµ РµРґРёРЅСЃС‚РІРµРЅРЅС‹Р№ СЃРїРѕСЃРѕР±, РµСЃР»Рё `windres` Р±С‹Р» РЅРµРґРѕСЃС‚СѓРїРµРЅ РїСЂРё СЃР±РѕСЂРєРµ).
pub fn enable_dpi_awareness() {
    // SAFETY: С„СѓРЅРєС†РёСЏ РЅРµ РїСЂРёРЅРёРјР°РµС‚ СѓРєР°Р·Р°С‚РµР»РµР№ Рё РЅРµ С‚СЂРµР±СѓРµС‚ РѕСЃРІРѕР±РѕР¶РґРµРЅРёСЏ СЂРµСЃСѓСЂСЃРѕРІ.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// dpi РѕСЃРЅРѕРІРЅРѕРіРѕ РјРѕРЅРёС‚РѕСЂР° (РґР»СЏ РїРµСЂРІРёС‡РЅРѕРіРѕ СЂР°Р·РјРµСЂР° РѕРєРЅР°).
pub fn system_dpi() -> u32 {
    // SAFETY: Р°СЂРіСѓРјРµРЅС‚РѕРІ РЅРµС‚.
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 {
        96
    } else {
        dpi
    }
}

/// dpi РѕРєРЅР° (РјРѕР¶РµС‚ РѕС‚Р»РёС‡Р°С‚СЊСЃСЏ РѕС‚ СЃРёСЃС‚РµРјРЅРѕРіРѕ РїСЂРё РїРµСЂРµРЅРѕСЃРµ РЅР° РґСЂСѓРіРѕР№ РјРѕРЅРёС‚РѕСЂ).
pub fn dpi_of(hwnd: HWND) -> u32 {
    // SAFETY: hwnd РїРѕР»СѓС‡РµРЅ РѕС‚ Win32 Рё Р¶РёРІС‘С‚, РїРѕРєР° РЅРµ СѓРЅРёС‡С‚РѕР¶РµРЅ.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        system_dpi()
    } else {
        dpi
    }
}

/// Р Р°Р·РјРµСЂ РєР»РёРµРЅС‚СЃРєРѕР№ РѕР±Р»Р°СЃС‚Рё РѕРєРЅР°.
pub fn client_size(hwnd: HWND) -> (i32, i32) {
    // SAFETY: RECT вЂ” РѕР±С‹С‡РЅР°СЏ СЃС‚СЂСѓРєС‚СѓСЂР°; GetClientRect Р·Р°РїРѕР»РЅСЏРµС‚ РµС‘ РїСЂРё РІР°Р»РёРґРЅРѕРј hwnd.
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetClientRect(hwnd, &mut rect) };
    if ok == 0 {
        (0, 0)
    } else {
        (rect.right - rect.left, rect.bottom - rect.top)
    }
}

/// РЎРѕР·РґР°С‚СЊ РґРѕС‡РµСЂРЅРµРµ РѕРєРЅРѕ (`STATIC`, `BUTTON`, вЂ¦) СЃ Р·Р°РґР°РЅРЅС‹Рј `id`.
pub fn create_child(parent: HWND, class: &str, text: &str, style: u32, id: usize) -> HWND {
    create_child_ex(parent, 0, class, text, style, id)
}

/// РўРѕ Р¶Рµ, РЅРѕ СЃ СЂР°СЃС€РёСЂРµРЅРЅС‹Рј СЃС‚РёР»РµРј (`WS_EX_CLIENTEDGE` РґР»СЏ СЃРїРёСЃРєР°).
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
    // SAFETY: СЃС‚СЂРѕРєРё Р¶РёРІСѓС‚ РґРѕ РєРѕРЅС†Р° РІС‹СЂР°Р¶РµРЅРёСЏ; parent РІР°Р»РёРґРµРЅ, id вЂ” РЅР°С€ РєРѕРЅС‚СЂРѕР».
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

/// РџРµСЂРµРјРµСЃС‚РёС‚СЊ/СЂР°СЃС‚СЏРЅСѓС‚СЊ РґРѕС‡РµСЂРЅРµРµ РѕРєРЅРѕ РїРѕ `id`.
pub fn move_child(parent: HWND, id: usize, rect: Rect) {
    // SAFETY: GetDlgItem РІРµСЂРЅС‘С‚ NULL, РµСЃР»Рё РєРѕРЅС‚СЂРѕР»Р° РЅРµС‚ вЂ” РїСЂРѕРІРµСЂСЏРµРј РґРѕ MoveWindow.
    let child = unsafe { GetDlgItem(parent, id as i32) };
    if child.is_null() {
        return;
    }
    // SAFETY: child РЅР°Р№РґРµРЅ РІ parent; РєРѕРѕСЂРґРёРЅР°С‚С‹ вЂ” РѕР±С‹С‡РЅС‹Рµ i32.
    unsafe {
        MoveWindow(child, rect.x, rect.y, rect.w, rect.h, 1);
    }
}

/// РџРѕРєР°Р·Р°С‚СЊ РѕРєРЅРѕ.
pub fn show(hwnd: HWND) {
    // SAFETY: hwnd СЃРѕР·РґР°РЅ Рё РµС‰С‘ РЅРµ СѓРЅРёС‡С‚РѕР¶РµРЅ.
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
    }
}

/// РЈРЅРёС‡С‚РѕР¶РёС‚СЊ РѕРєРЅРѕ (РїРѕСЃР»Рµ СЌС‚РѕРіРѕ СЃРѕРѕР±С‰РµРЅРёРµ `WM_DESTROY` Р·Р°РІРµСЂС€РёС‚ С†РёРєР»).
pub fn destroy(hwnd: HWND) {
    // SAFETY: РІС‹Р·С‹РІР°РµС‚СЃСЏ РїРѕ WM_COMMAND/WM_CLOSE РґР»СЏ Р¶РёРІРѕРіРѕ РѕРєРЅР°.
    unsafe {
        DestroyWindow(hwnd);
    }
}

/// Р—Р°СЂРµРіРёСЃС‚СЂРёСЂРѕРІР°С‚СЊ РєР»Р°СЃСЃС‹ СЃС‚Р°РЅРґР°СЂС‚РЅС‹С… РєРѕРЅС‚СЂРѕР»РѕРІ (СЃРїРёСЃРѕРє, РєРЅРѕРїРєРё).
pub fn init_common_controls() {
    let mut settings = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_LISTVIEW_CLASSES | ICC_STANDARD_CLASSES,
    };
    // SAFETY: СЃС‚СЂСѓРєС‚СѓСЂР° РёРЅРёС†РёР°Р»РёР·РёСЂРѕРІР°РЅР° РїРѕР»РЅРѕСЃС‚СЊСЋ, СЂР°Р·РјРµСЂ РїРµСЂРµРґР°РЅ РІРµСЂРЅРѕ.
    unsafe {
        InitCommonControlsEx(&mut settings);
    }
}

/// Р”РѕС‡РµСЂРЅРµРµ РѕРєРЅРѕ РїРѕ `id` (`HWND::default()`, РµСЃР»Рё РєРѕРЅС‚СЂРѕР»Р° РЅРµС‚).
pub fn child(parent: HWND, id: usize) -> HWND {
    // SAFETY: parent вЂ” Р¶РёРІРѕРµ РѕРєРЅРѕ; РїСЂРё РѕС‚СЃСѓС‚СЃС‚РІРёРё РєРѕРЅС‚СЂРѕР»Р° РІРµСЂРЅС‘С‚СЃСЏ NULL.
    unsafe { GetDlgItem(parent, id as i32) }
}

/// Р—Р°РјРµРЅРёС‚СЊ С‚РµРєСЃС‚ РєРѕРЅС‚СЂРѕР»Р°.
pub fn set_text(hwnd: HWND, text: &str) {
    if hwnd.is_null() {
        return;
    }
    let value = wide(text);
    // SAFETY: СЃС‚СЂРѕРєР° Р¶РёРІС‘С‚ РґРѕ РєРѕРЅС†Р° РІС‹Р·РѕРІР°; hwnd РїСЂРѕРІРµСЂРµРЅ РЅР° NULL.
    unsafe {
        SetWindowTextW(hwnd, value.as_ptr());
    }
}

/// Р Р°Р·СЂРµС€РёС‚СЊ РїСЂРёС‘Рј РїРµСЂРµС‚Р°С‰РµРЅРЅС‹С… С„Р°Р№Р»РѕРІ (`WM_DROPFILES`).
pub fn accept_dropped_files(hwnd: HWND) {
    // SAFETY: hwnd вЂ” Р¶РёРІРѕРµ РѕРєРЅРѕ.
    unsafe {
        DragAcceptFiles(hwnd, 1);
    }
}

/// Р¤Р°Р№Р»С‹ РёР· СЃРѕРѕР±С‰РµРЅРёСЏ `WM_DROPFILES` (СЃРїРёСЃРѕРє РѕСЃРІРѕР±РѕР¶РґР°РµС‚СЃСЏ С‡РµСЂРµР· `DragFinish`).
pub fn dropped_files(hdrop: *mut c_void) -> Vec<PathBuf> {
    // SAFETY: hdrop РІР°Р»РёРґРµРЅ РІ РїСЂРµРґРµР»Р°С… РѕР±СЂР°Р±РѕС‚РєРё WM_DROPFILES.
    let count = unsafe { DragQueryFileW(hdrop, u32::MAX, ptr::null_mut(), 0) };
    let mut files = Vec::with_capacity(count as usize);
    for index in 0..count {
        // SAFETY: С‚РѕС‚ Р¶Рµ hdrop; NULL Рё РЅСѓР»РµРІР°СЏ РґР»РёРЅР° вЂ” Р·Р°РїСЂРѕСЃ СЂР°Р·РјРµСЂР°.
        let length = unsafe { DragQueryFileW(hdrop, index, ptr::null_mut(), 0) };
        if length == 0 {
            continue;
        }
        let mut buffer = vec![0u16; length as usize + 1];
        // SAFETY: Р±СѓС„РµСЂ РґРѕСЃС‚Р°С‚РѕС‡РЅРѕРіРѕ СЂР°Р·РјРµСЂР°, hdrop РІР°Р»РёРґРµРЅ.
        let written =
            unsafe { DragQueryFileW(hdrop, index, buffer.as_mut_ptr(), buffer.len() as u32) };
        if written > 0 {
            files.push(PathBuf::from(String::from_utf16_lossy(&buffer[..written as usize])));
        }
    }
    // SAFETY: РѕСЃРІРѕР±РѕР¶РґР°С‚СЊ hdrop РЅСѓР¶РЅРѕ СЂРѕРІРЅРѕ РѕРґРёРЅ СЂР°Р· РїРѕСЃР»Рµ С‡С‚РµРЅРёСЏ.
    unsafe {
        DragFinish(hdrop);
    }
    files
}

/// Р”РёР°Р»РѕРі РІС‹Р±РѕСЂР° С„Р°Р№Р»РѕРІ (`GetOpenFileNameW`, РјСѓР»СЊС‚РёРІС‹Р±РѕСЂ).
///
/// `filter` вЂ” РїР°СЂС‹ В«РѕРїРёСЃР°РЅРёРµ, РјР°СЃРєР°В» (`"РћР±Р»Р°РєР° С‚РѕС‡РµРє", "*.las;*.laz"`).
/// РџСѓСЃС‚РѕР№ РІРµРєС‚РѕСЂ РѕР·РЅР°С‡Р°РµС‚ РѕС‚РјРµРЅСѓ.
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

    // Р‘СѓС„РµСЂ РїРѕРґ СЃРїРёСЃРѕРє С„Р°Р№Р»РѕРІ: РїСЂРё РјСѓР»СЊС‚РёРІС‹Р±РѕСЂРµ Win32 РїРёС€РµС‚ РєР°С‚Р°Р»РѕРі Рё РёРјРµРЅР°.
    let mut buffer = vec![0u16; 64 * 1024];
    // SAFETY: OPENFILENAMEW вЂ” РѕР±С‹С‡РЅР°СЏ СЃС‚СЂСѓРєС‚СѓСЂР°, РЅСѓР»Рё вЂ” РєРѕСЂСЂРµРєС‚РЅРѕРµ РЅР°С‡Р°Р»СЊРЅРѕРµ Р·РЅР°С‡РµРЅРёРµ.
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

    // SAFETY: РІСЃРµ СѓРєР°Р·Р°С‚РµР»Рё РІ dialog РІР°Р»РёРґРЅС‹ Рё Р¶РёРІСѓС‚ РґРѕ РєРѕРЅС†Р° РІС‹Р·РѕРІР°.
    let accepted = unsafe { GetOpenFileNameW(&mut dialog) };
    if accepted == 0 {
        return Vec::new();
    }
    crate::model::parse_open_file_buffer(&buffer)
}

/// Р”РёР°Р»РѕРі РІС‹Р±РѕСЂР° РєР°С‚Р°Р»РѕРіР° (`SHBrowseForFolderW`).
pub fn browse_folder(owner: HWND, caption: &str) -> Option<PathBuf> {
    let caption_w = wide(caption);
    let mut display = vec![0u16; 260];
    // SAFETY: BROWSEINFOW вЂ” РѕР±С‹С‡РЅР°СЏ СЃС‚СЂСѓРєС‚СѓСЂР°; РЅСѓР»Рё вЂ” РєРѕСЂСЂРµРєС‚РЅРѕРµ РЅР°С‡Р°Р»СЊРЅРѕРµ Р·РЅР°С‡РµРЅРёРµ.
    let mut info: BROWSEINFOW = unsafe { std::mem::zeroed() };
    info.hwndOwner = owner;
    info.pszDisplayName = display.as_mut_ptr();
    info.lpszTitle = caption_w.as_ptr();
    info.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX;

    // SAFETY: СЃС‚СЂСѓРєС‚СѓСЂР° Р·Р°РїРѕР»РЅРµРЅР°; РІРѕР·РІСЂР°С‰С‘РЅРЅС‹Р№ PIDL РІС‹РґРµР»РµРЅ РѕР±РѕР»РѕС‡РєРѕР№.
    let pidl = unsafe { SHBrowseForFolderW(&info) };
    if pidl.is_null() {
        return None;
    }
    let mut path = vec![0u16; 32 * 1024];
    // SAFETY: pidl РІР°Р»РёРґРµРЅ, Р±СѓС„РµСЂ РґРѕСЃС‚Р°С‚РѕС‡РЅРѕРіРѕ СЂР°Р·РјРµСЂР°.
    let ok = unsafe { SHGetPathFromIDListW(pidl, path.as_mut_ptr()) };
    // SAFETY: PIDL РІС‹РґРµР»РµРЅ РѕР±РѕР»РѕС‡РєРѕР№ С‡РµСЂРµР· CoTaskMemAlloc вЂ” РѕСЃРІРѕР±РѕР¶РґР°РµРј РѕРґРёРЅ СЂР°Р·.
    unsafe {
        CoTaskMemFree(pidl as *const c_void);
    }
    if ok == 0 {
        return None;
    }
    let length = path.iter().position(|&value| value == 0).unwrap_or(0);
    Some(PathBuf::from(String::from_utf16_lossy(&path[..length])))
}

/// РЎРѕР·РґР°С‚СЊ РєРѕР»РѕРЅРєРё СЃРїРёСЃРєР° (С€РёСЂРёРЅС‹ РёР· [`crate::model::COLUMNS`], РјР°СЃС€С‚Р°Р± вЂ” РїРѕ dpi).
pub fn list_init(list: HWND, dpi: u32) {
    if list.is_null() {
        return;
    }
    let extended = LVS_EX_FULLROWSELECT | LVS_EX_GRIDLINES;
    // SAFETY: list вЂ” Р¶РёРІРѕР№ РєРѕРЅС‚СЂРѕР»; С„Р»Р°РіРё вЂ” РѕР±С‹С‡РЅС‹Рµ u32.
    unsafe {
        SendMessageW(list, LVM_SETEXTENDEDLISTVIEWSTYLE, extended as usize, extended as isize);
    }
    for (index, (title, width)) in COLUMNS.iter().enumerate() {
        let text = wide(title);
        // SAFETY: LVCOLUMNW вЂ” РѕР±С‹С‡РЅР°СЏ СЃС‚СЂСѓРєС‚СѓСЂР°; РЅСѓР»Рё РєРѕСЂСЂРµРєС‚РЅС‹, РїРѕР»СЏ Р·Р°РґР°СЋС‚СЃСЏ РЅРёР¶Рµ.
        let mut column: LVCOLUMNW = unsafe { std::mem::zeroed() };
        column.mask = LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM;
        column.pszText = text.as_ptr() as *mut u16;
        column.cchTextMax = text.len() as i32;
        column.cx = crate::model::scale(*width, dpi);
        column.iSubItem = index as i32;
        // SAFETY: Win32 РєРѕРїРёСЂСѓРµС‚ СЃС‚СЂРѕРєСѓ Рё СЃС‚СЂСѓРєС‚СѓСЂСѓ РІРѕ РІСЂРµРјСЏ РІС‹Р·РѕРІР°.
        unsafe {
            SendMessageW(list, LVM_INSERTCOLUMNW, index, &column as *const _ as isize);
        }
    }
}

/// РћС‡РёСЃС‚РёС‚СЊ СЃРїРёСЃРѕРє С„Р°Р№Р»РѕРІ.
pub fn list_clear(list: HWND) {
    // SAFETY: list вЂ” Р¶РёРІРѕР№ РєРѕРЅС‚СЂРѕР».
    unsafe {
        SendMessageW(list, LVM_DELETEALLITEMS, 0, 0);
    }
}

/// Р”РѕР±Р°РІРёС‚СЊ СЃС‚СЂРѕРєСѓ: `cells[0]` вЂ” РїРµСЂРІС‹Р№ СЃС‚РѕР»Р±РµС†, РѕСЃС‚Р°Р»СЊРЅС‹Рµ вЂ” РїРѕРґСЌР»РµРјРµРЅС‚С‹.
pub fn list_add_row(list: HWND, row: usize, cells: &[String]) {
    let Some(first_cell) = cells.first() else {
        return;
    };
    let first = wide(first_cell);
    // SAFETY: LVITEMW вЂ” РѕР±С‹С‡РЅР°СЏ СЃС‚СЂСѓРєС‚СѓСЂР°; РЅСѓР»Рё РєРѕСЂСЂРµРєС‚РЅС‹, РїРѕР»СЏ Р·Р°РґР°СЋС‚СЃСЏ РЅРёР¶Рµ.
    let mut item: LVITEMW = unsafe { std::mem::zeroed() };
    item.mask = LVIF_TEXT;
    item.iItem = row as i32;
    item.iSubItem = 0;
    item.pszText = first.as_ptr() as *mut u16;
    item.cchTextMax = first.len() as i32;
    // SAFETY: Win32 РєРѕРїРёСЂСѓРµС‚ СЃС‚СЂРѕРєСѓ Рё СЃС‚СЂСѓРєС‚СѓСЂСѓ РІРѕ РІСЂРµРјСЏ РІС‹Р·РѕРІР°.
    unsafe {
        SendMessageW(list, LVM_INSERTITEMW, 0, &item as *const _ as isize);
    }

    // РЎС‚СЂРѕРєРё РґР»СЏ РїРѕРґСЌР»РµРјРµРЅС‚РѕРІ Р¶РёРІСѓС‚ РґРѕ РєРѕРЅС†Р° С„СѓРЅРєС†РёРё вЂ” Win32 РєРѕРїРёСЂСѓРµС‚ РёС… СЃР°Рј.
    let rest: Vec<Vec<u16>> = cells[1..].iter().map(|cell| wide(cell)).collect();
    for (offset, text) in rest.iter().enumerate() {
        // SAFETY: С‚Р° Р¶Рµ СЃС‚СЂСѓРєС‚СѓСЂР°, С‡С‚Рѕ РІС‹С€Рµ; РїР°РјСЏС‚СЊ РЅСѓР»РµРІР°СЏ.
        let mut sub: LVITEMW = unsafe { std::mem::zeroed() };
        sub.iSubItem = (offset + 1) as i32;
        sub.pszText = text.as_ptr() as *mut u16;
        // SAFETY: Win32 РєРѕРїРёСЂСѓРµС‚ СЃС‚СЂРѕРєСѓ РІРѕ РІСЂРµРјСЏ РІС‹Р·РѕРІР°.
        unsafe {
            SendMessageW(list, LVM_SETITEMTEXTW, row, &sub as *const _ as isize);
        }
    }
}

/// РРЅРґРµРєСЃ РІС‹РґРµР»РµРЅРЅРѕР№ СЃС‚СЂРѕРєРё СЃРїРёСЃРєР°.
pub fn list_selected(list: HWND) -> Option<usize> {
    // SAFETY: LVM_GETNEXTITEM СЃ wParam = -1 РёС‰РµС‚ СЃ РЅР°С‡Р°Р»Р°; РІРµСЂРЅС‘С‚ -1, РµСЃР»Рё РїСѓСЃС‚Рѕ.
    let index = unsafe { SendMessageW(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize) };
    if index < 0 {
        None
    } else {
        Some(index as usize)
    }
}

/// РўРµРєСЃС‚ РєРѕРЅС‚СЂРѕР»Р° (`Edit`, `Static`, `ComboBox`).
pub fn text_of(hwnd: HWND) -> String {
    if hwnd.is_null() {
        return String::new();
    }
    // SAFETY: hwnd вЂ” Р¶РёРІРѕР№ РєРѕРЅС‚СЂРѕР».
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    // SAFETY: Р±СѓС„РµСЂ РґРѕСЃС‚Р°С‚РѕС‡РЅРѕРіРѕ СЂР°Р·РјРµСЂР° (length + NUL).
    let written = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    if written <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..written as usize])
}

/// Р—Р°РїРѕР»РЅРёС‚СЊ РІС‹РїР°РґР°СЋС‰РёР№ СЃРїРёСЃРѕРє Рё РІС‹Р±СЂР°С‚СЊ СЌР»РµРјРµРЅС‚.
pub fn combo_fill(combo: HWND, items: &[&str], selected: usize) {
    if combo.is_null() {
        return;
    }
    for item in items {
        let text = wide(item);
        // SAFETY: Win32 РєРѕРїРёСЂСѓРµС‚ СЃС‚СЂРѕРєСѓ РІРѕ РІСЂРµРјСЏ РІС‹Р·РѕРІР°.
        unsafe {
            SendMessageW(combo, CB_ADDSTRING, 0, text.as_ptr() as isize);
        }
    }
    combo_select(combo, selected.min(items.len().saturating_sub(1)));
}

/// Р’С‹Р±СЂР°С‚СЊ СЌР»РµРјРµРЅС‚ РІС‹РїР°РґР°СЋС‰РµРіРѕ СЃРїРёСЃРєР°.
pub fn combo_select(combo: HWND, index: usize) {
    if combo.is_null() {
        return;
    }
    // SAFETY: РёРЅРґРµРєСЃ РЅРµ РІС‹С…РѕРґРёС‚ Р·Р° РіСЂР°РЅРёС†С‹ РґРѕР±Р°РІР»РµРЅРЅС‹С… СЃС‚СЂРѕРє (РїСЂРѕРІРµСЂСЏРµС‚ РІС‹Р·С‹РІР°СЋС‰РёР№).
    unsafe {
        SendMessageW(combo, CB_SETCURSEL, index, 0);
    }
}

/// Р’С‹Р±СЂР°РЅРЅС‹Р№ СЌР»РµРјРµРЅС‚ СЃРїРёСЃРєР°.
pub fn combo_selected(combo: HWND) -> Option<usize> {
    if combo.is_null() {
        return None;
    }
    // SAFETY: CB_GETCURSEL РІРѕР·РІСЂР°С‰Р°РµС‚ -1, РµСЃР»Рё РЅРёС‡РµРіРѕ РЅРµ РІС‹Р±СЂР°РЅРѕ.
    let index = unsafe { SendMessageW(combo, CB_GETCURSEL, 0, 0) };
    if index < 0 {
        None
    } else {
        Some(index as usize)
    }
}

/// РџРѕСЃС‚Р°РІРёС‚СЊ/СЃРЅСЏС‚СЊ С„Р»Р°Р¶РѕРє.
pub fn check_set(control: HWND, checked: bool) {
    if control.is_null() {
        return;
    }
    let state = if checked { BST_CHECKED as usize } else { 0 };
    // SAFETY: control вЂ” С„Р»Р°Р¶РѕРє (BS_AUTOCHECKBOX), СЃРѕСЃС‚РѕСЏРЅРёРµ вЂ” РѕР±С‹С‡РЅРѕРµ С‡РёСЃР»Рѕ.
    unsafe {
        SendMessageW(control, BM_SETCHECK, state, 0);
    }
}

/// РЎС‚РѕРёС‚ Р»Рё С„Р»Р°Р¶РѕРє.
pub fn check_get(control: HWND) -> bool {
    if control.is_null() {
        return false;
    }
    // SAFETY: control вЂ” С„Р»Р°Р¶РѕРє; BM_GETCHECK РЅРµ РїРёС€РµС‚ РІ РїР°РјСЏС‚СЊ.
    let state = unsafe { SendMessageW(control, BM_GETCHECK, 0, 0) };
    state == BST_CHECKED as isize
}

/// РљР»СЋС‡ СЂРµРµСЃС‚СЂР° СЃ РЅР°СЃС‚СЂРѕР№РєР°РјРё РѕРєРЅР°.
const SETTINGS_KEY: &str = "Software\\rcsconv\\gui";

/// РћС‚РєСЂС‹С‚СЊ (СЃРѕР·РґР°С‚СЊ) РєР»СЋС‡ РЅР°СЃС‚СЂРѕРµРє РІ `HKCU`.
fn open_settings() -> Option<HKEY> {
    let subkey = wide(SETTINGS_KEY);
    let mut key: HKEY = ptr::null_mut();
    // SAFETY: subkey Р¶РёРІС‘С‚ РґРѕ РєРѕРЅС†Р° РІС‹Р·РѕРІР°; security attributes РЅРµ РЅСѓР¶РЅС‹ (NULL).
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

/// РЎРѕС…СЂР°РЅРёС‚СЊ РЅР°СЃС‚СЂРѕР№РєРё РІ `HKCU\Software\rcsconv\gui` (РѕС€РёР±РєРё РјРѕР»С‡Р° РёРіРЅРѕСЂРёСЂСѓСЋС‚СЃСЏ).
pub fn registry_save(values: &[(String, String)]) {
    let Some(key) = open_settings() else {
        return;
    };
    for (name, value) in values {
        let name_w = wide(name);
        let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = std::mem::size_of_val(data.as_slice()) as u32;
        // SAFETY: РёРјСЏ Рё РґР°РЅРЅС‹Рµ Р¶РёРІСѓС‚ РґРѕ РєРѕРЅС†Р° РІС‹Р·РѕРІР°; С‚РёРї вЂ” REG_SZ, СЂР°Р·РјРµСЂ РІ Р±Р°Р№С‚Р°С….
        unsafe {
            RegSetValueExW(key, name_w.as_ptr(), 0, REG_SZ, data.as_ptr() as *const u8, bytes);
        }
    }
    // SAFETY: РєР»СЋС‡ РїРѕР»СѓС‡РµРЅ РёР· RegCreateKeyExW Рё РѕСЃРІРѕР±РѕР¶РґР°РµС‚СЃСЏ РѕРґРёРЅ СЂР°Р·.
    unsafe {
        RegCloseKey(key);
    }
}

/// РџСЂРѕС‡РёС‚Р°С‚СЊ СЃС‚СЂРѕРєРѕРІС‹Рµ Р·РЅР°С‡РµРЅРёСЏ РёР· `HKCU\Software\rcsconv\gui`.
pub fn registry_load(names: &[&str]) -> Vec<(String, String)> {
    let Some(key) = open_settings() else {
        return Vec::new();
    };
    let mut values = Vec::new();
    for name in names {
        let name_w = wide(name);
        let mut kind: u32 = 0;
        let mut size: u32 = 0;
        // SAFETY: РїРµСЂРІС‹Р№ РІС‹Р·РѕРІ вЂ” С‚РѕР»СЊРєРѕ СЂР°Р·РјРµСЂ РґР°РЅРЅС‹С….
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
        // SAFETY: Р±СѓС„РµСЂ РґРѕСЃС‚Р°С‚РѕС‡РЅРѕРіРѕ СЂР°Р·РјРµСЂР°, СЂР°Р·РјРµСЂ РїРµСЂРµРґР°РЅ РІ Р±Р°Р№С‚Р°С….
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
    // SAFETY: РєР»СЋС‡ РѕСЃРІРѕР±РѕР¶РґР°РµС‚СЃСЏ РѕРґРёРЅ СЂР°Р·.
    unsafe {
        RegCloseKey(key);
    }
    values
}

/// Р—Р°РґР°С‚СЊ РґРёР°РїР°Р·РѕРЅ РїРѕР»РѕСЃС‹ РїСЂРѕРіСЂРµСЃСЃР° (`0вЂ¦max`).
pub fn progress_range(bar: HWND, max: u32) {
    if bar.is_null() {
        return;
    }
    // SAFETY: bar вЂ” Р¶РёРІР°СЏ РїРѕР»РѕСЃР° РїСЂРѕРіСЂРµСЃСЃР°; РґРёР°РїР°Р·РѕРЅ вЂ” РѕР±С‹С‡РЅС‹Рµ С‡РёСЃР»Р°.
    unsafe {
        SendMessageW(bar, PBM_SETRANGE32, 0, max as isize);
    }
}

/// РЈСЃС‚Р°РЅРѕРІРёС‚СЊ РїРѕР»РѕР¶РµРЅРёРµ РїРѕР»РѕСЃС‹ РїСЂРѕРіСЂРµСЃСЃР°.
pub fn progress_set(bar: HWND, value: u32) {
    if bar.is_null() {
        return;
    }
    // SAFETY: bar вЂ” Р¶РёРІР°СЏ РїРѕР»РѕСЃР° РїСЂРѕРіСЂРµСЃСЃР°.
    unsafe {
        SendMessageW(bar, PBM_SETPOS, value as usize, 0);
    }
}

/// РћС‚РєСЂС‹С‚СЊ РїСѓС‚СЊ РІ РїСЂРѕРІРѕРґРЅРёРєРµ (РёР»Рё СЃРІСЏР·Р°РЅРЅРѕР№ РїСЂРѕРіСЂР°РјРјРµ).
pub fn shell_open(path: &Path) -> bool {
    let operation = wide("open");
    let target = wide(&path.display().to_string());
    // SAFETY: СЃС‚СЂРѕРєРё Р¶РёРІСѓС‚ РґРѕ РєРѕРЅС†Р° РІС‹Р·РѕРІР°; РєР°С‚Р°Р»РѕРі/С„Р°Р№Р» РѕС‚РєСЂС‹РІР°РµС‚СЃСЏ Р°СЃСЃРѕС†РёР°С†РёРµР№.
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
    // ShellExecuteW РІРѕР·РІСЂР°С‰Р°РµС‚ Р·РЅР°С‡РµРЅРёРµ > 32 РїСЂРё СѓСЃРїРµС…Рµ (С‚Р°Рє С‚СЂРµР±СѓРµС‚ РґРѕРєСѓРјРµРЅС‚Р°С†РёСЏ).
    result as isize > 32
}

/// РћР±РЅРѕРІРёС‚СЊ РѕРґРЅСѓ СЏС‡РµР№РєСѓ СЃРїРёСЃРєР° (`row` вЂ” СЃС‚СЂРѕРєР°, `column` вЂ” СЃС‚РѕР»Р±РµС†).
pub fn list_set_cell(list: HWND, row: usize, column: usize, text: &str) {
    if list.is_null() {
        return;
    }
    let value = wide(text);
    // SAFETY: LVITEMW вЂ” РѕР±С‹С‡РЅР°СЏ СЃС‚СЂСѓРєС‚СѓСЂР°; РЅСѓР»Рё РєРѕСЂСЂРµРєС‚РЅС‹, РїРѕР»СЏ Р·Р°РґР°СЋС‚СЃСЏ РЅРёР¶Рµ.
    let mut item: LVITEMW = unsafe { std::mem::zeroed() };
    item.iSubItem = column as i32;
    item.pszText = value.as_ptr() as *mut u16;
    // SAFETY: Win32 РєРѕРїРёСЂСѓРµС‚ СЃС‚СЂРѕРєСѓ РІРѕ РІСЂРµРјСЏ РІС‹Р·РѕРІР°.
    unsafe {
        SendMessageW(list, LVM_SETITEMTEXTW, row, &item as *const _ as isize);
    }
}
