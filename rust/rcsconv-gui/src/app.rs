//! Главное окно: регистрация класса, контролы, цикл сообщений.
//!
//! Фаза G1 (`docs/ROADMAP.md` §4.1): окно открывается, элементы размещаются по
//! `WM_SIZE`, «Закрыть» и крестик завершают приложение. Очередь файлов,
//! параметры и сама конвертация подключаются в G2…G4.

use std::ptr;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{UpdateWindow, HBRUSH};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::model::{self, Id};
use crate::win32;

/// `COLOR_WINDOW` (5) + 1: так Win32 просит системную кисть фона окна.
const BACKGROUND_BRUSH: u32 = 5 + 1;

/// Запустить интерфейс; вернуть код выхода процесса.
pub fn run() -> u8 {
    win32::enable_dpi_awareness();

    let instance = win32::instance();
    let class_name = win32::wide(model::WINDOW_CLASS);
    let title = win32::wide(model::WINDOW_TITLE);

    // SAFETY: WNDCLASSW — обычная структура; заполняем нужные поля, остальные нули.
    let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
    class.style = CS_HREDRAW | CS_VREDRAW;
    class.lpfnWndProc = Some(wndproc);
    class.hInstance = instance;
    // SAFETY: IDC_ARROW — стандартный курсор; hInstance для него не используется.
    class.hCursor = unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) };
    class.hbrBackground = BACKGROUND_BRUSH as HBRUSH;
    class.lpszClassName = class_name.as_ptr();

    // SAFETY: все указатели в class валидны и живут до конца функции.
    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        show_error("не удалось зарегистрировать класс окна");
        return 2;
    }

    let dpi = win32::system_dpi();
    // SAFETY: строки живут до конца выражения, instance получен из системы.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            model::scale(960, dpi),
            model::scale(640, dpi),
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        )
    };
    if hwnd.is_null() {
        show_error("не удалось создать главное окно");
        return 3;
    }

    win32::show(hwnd);
    // SAFETY: hwnd только что создан и ещё не уничтожен.
    unsafe {
        UpdateWindow(hwnd);
    }

    message_loop()
}

/// Цикл сообщений до `WM_QUIT`.
fn message_loop() -> u8 {
    // SAFETY: MSG — обычная структура Win32; нули — корректное начальное значение.
    let mut message: MSG = unsafe { std::mem::zeroed() };
    loop {
        // SAFETY: указатель на message валиден, фильтр — «все сообщения».
        let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        match result {
            0 => return message.wParam as u8,
            -1 => return 1,
            _ => {}
        }
        // SAFETY: message заполнено GetMessageW.
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Оконная процедура: создание контролов, раскладка, закрытие.
///
/// # Safety
///
/// Вызывается только системой; `hwnd` — живое окно нашего класса.
unsafe extern "system" fn wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            create_controls(hwnd);
            layout_controls(hwnd);
            0
        }
        WM_SIZE => {
            layout_controls(hwnd);
            0
        }
        WM_DPICHANGED => {
            // lparam — предлагаемый прямоугольник окна для нового монитора.
            let suggested = lparam as *const RECT;
            if !suggested.is_null() {
                // SAFETY: Win32 передаёт валидный указатель на RECT в этом сообщении.
                let rect = unsafe { *suggested };
                // SAFETY: hwnd жив; координаты взяты из предложенного прямоугольника.
                unsafe {
                    SetWindowPos(
                        hwnd,
                        ptr::null_mut(),
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
            layout_controls(hwnd);
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as usize;
            if id == Id::Close.raw() {
                win32::destroy(hwnd);
            }
            0
        }
        WM_CLOSE => {
            win32::destroy(hwnd);
            0
        }
        WM_DESTROY => {
            // SAFETY: WM_DESTROY приходит последним для этого потока.
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => {
            // SAFETY: DefWindowProcW — обработчик по умолчанию для остальных сообщений.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
    }
}

/// Создать элементы окна-каркаса.
fn create_controls(hwnd: HWND) {
    let version = model::version_line();
    win32::create_child(hwnd, "STATIC", &version, WS_CHILD | WS_VISIBLE, Id::Title.raw());
    win32::create_child(hwnd, "STATIC", model::HINT, WS_CHILD | WS_VISIBLE, Id::Hint.raw());
    win32::create_child(
        hwnd,
        "BUTTON",
        "Закрыть",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
        Id::Close.raw(),
    );
}

/// Разложить контролы по текущему размеру клиентской области и dpi.
fn layout_controls(hwnd: HWND) {
    let (width, height) = win32::client_size(hwnd);
    let dpi = win32::dpi_of(hwnd);
    let (title, hint, close) = model::layout(width, height, dpi);
    win32::move_child(hwnd, Id::Title.raw(), title);
    win32::move_child(hwnd, Id::Hint.raw(), hint);
    win32::move_child(hwnd, Id::Close.raw(), close);
}

/// Показать ошибку в модальном окне (используется до появления главного окна).
fn show_error(text: &str) {
    let text = win32::wide(text);
    let caption = win32::wide("rcsconv-gui");
    // SAFETY: обе строки живут до возврата из MessageBoxW.
    unsafe {
        MessageBoxW(ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_ICONERROR | MB_OK);
    }
}
