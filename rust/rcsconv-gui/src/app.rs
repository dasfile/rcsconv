//! Главное окно: регистрация класса, очередь файлов, цикл сообщений.
//!
//! Фаза G2 (`docs/ROADMAP.md` §4.1): список файлов (`SysListView32`), выбор
//! файлов и каталогов диалогами, перетаскивание, «Убрать»/«Очистить».
//! Параметры и конвертация — G3/G4.

use std::ffi::c_void;
use std::ptr;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{UpdateWindow, HBRUSH};
use windows_sys::Win32::UI::Controls::{LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::model::{self, Id, Queue};
use crate::win32;

/// `COLOR_WINDOW` (5) + 1: так Win32 просит системную кисть фона окна.
const BACKGROUND_BRUSH: u32 = 5 + 1;

/// Фильтр диалога выбора файлов.
const OPEN_FILTER: [(&str, &str); 2] =
    [("Облака точек", "*.las;*.laz;*.pts"), ("Все файлы", "*.*")];

/// Состояние окна: живёт в `GWLP_USERDATA` главного окна.
struct State {
    queue: Queue,
}

impl State {
    fn new() -> Box<State> {
        Box::new(State { queue: Queue::new() })
    }
}

/// Запустить интерфейс; вернуть код выхода процесса.
pub fn run() -> u8 {
    win32::enable_dpi_awareness();
    win32::init_common_controls();

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
            model::scale(980, dpi),
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

    // Владелец состояния — окно; освобождается в WM_DESTROY.
    let state = Box::into_raw(State::new());
    // SAFETY: hwnd жив; указатель на Box не освобождается до WM_DESTROY.
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
    }
    refresh(hwnd);

    win32::show(hwnd);
    // SAFETY: hwnd создан и ещё не уничтожен.
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

/// Указатель на состояние окна (NULL до `run()` и после `WM_DESTROY`).
fn state_ptr(hwnd: HWND) -> *mut State {
    // SAFETY: GWLP_USERDATA заполняется нами в run(); значение — указатель на Box.
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State }
}

/// Выполнить действие над состоянием окна, если оно уже создано.
fn with_state(hwnd: HWND, action: impl FnOnce(&mut State)) {
    let pointer = state_ptr(hwnd);
    if pointer.is_null() {
        return;
    }
    // SAFETY: указатель валиден, пока окно живо; все вызовы идут из UI-потока.
    let state = unsafe { &mut *pointer };
    action(state);
}

/// Забрать состояние при уничтожении окна.
fn take_state(hwnd: HWND) {
    let pointer = state_ptr(hwnd);
    if pointer.is_null() {
        return;
    }
    // SAFETY: указатель получен из Box::into_raw в run() и больше не используется.
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        drop(Box::from_raw(pointer));
    }
}

/// Оконная процедура: контролы, раскладка, команды, перетаскивание.
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
            win32::accept_dropped_files(hwnd);
            create_controls(hwnd);
            layout_controls(hwnd);
            0
        }
        WM_SIZE => {
            layout_controls(hwnd);
            0
        }
        WM_GETMINMAXINFO => {
            // Не даём сжать окно меньше минимального размера раскладки.
            let info = lparam as *mut MINMAXINFO;
            if !info.is_null() {
                let dpi = win32::dpi_of(hwnd);
                // SAFETY: Win32 передаёт валидный указатель на MINMAXINFO в этом сообщении.
                let info = unsafe { &mut *info };
                info.ptMinTrackSize.x =
                    model::scale(model::MIN_SIZE_96.0, dpi) + model::scale(32, dpi);
                info.ptMinTrackSize.y =
                    model::scale(model::MIN_SIZE_96.1, dpi) + model::scale(96, dpi);
            }
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
            handle_command(hwnd, (wparam & 0xFFFF) as usize);
            0
        }
        WM_DROPFILES => {
            handle_drop(hwnd, lparam as *mut c_void);
            0
        }
        WM_CLOSE => {
            win32::destroy(hwnd);
            0
        }
        WM_DESTROY => {
            take_state(hwnd);
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

/// Создать элементы окна.
fn create_controls(hwnd: HWND) {
    let version = model::version_line();
    win32::create_child(hwnd, "STATIC", &version, WS_CHILD | WS_VISIBLE, Id::Title.raw());
    win32::create_child(hwnd, "STATIC", model::HINT_EMPTY, WS_CHILD | WS_VISIBLE, Id::Hint.raw());

    let list_style =
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS;
    let list = win32::create_child_ex(
        hwnd,
        WS_EX_CLIENTEDGE,
        "SysListView32",
        "",
        list_style,
        Id::List.raw(),
    );
    win32::list_init(list, win32::dpi_of(hwnd));

    for (id, text) in [
        (Id::AddFiles, "Добавить файлы…"),
        (Id::AddFolder, "Добавить папку…"),
        (Id::Remove, "Убрать"),
        (Id::Clear, "Очистить"),
        (Id::Close, "Закрыть"),
    ] {
        win32::create_child(
            hwnd,
            "BUTTON",
            text,
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
            id.raw(),
        );
    }
}

/// Разложить контролы по текущему размеру клиентской области и dpi.
fn layout_controls(hwnd: HWND) {
    let (width, height) = win32::client_size(hwnd);
    let dpi = win32::dpi_of(hwnd);
    let layout = model::layout(width, height, dpi);
    for (id, rect) in [
        (Id::Title, layout.title),
        (Id::Hint, layout.hint),
        (Id::List, layout.list),
        (Id::AddFiles, layout.add_files),
        (Id::AddFolder, layout.add_folder),
        (Id::Remove, layout.remove),
        (Id::Clear, layout.clear),
        (Id::Close, layout.close),
    ] {
        win32::move_child(hwnd, id.raw(), rect);
    }
}

/// Обработать нажатие кнопки.
fn handle_command(hwnd: HWND, id: usize) {
    if id == Id::AddFiles.raw() {
        add_files(hwnd);
    } else if id == Id::AddFolder.raw() {
        add_folder(hwnd);
    } else if id == Id::Remove.raw() {
        remove_selected(hwnd);
    } else if id == Id::Clear.raw() {
        with_state(hwnd, |state| state.queue.clear());
        refresh(hwnd);
    } else if id == Id::Close.raw() {
        win32::destroy(hwnd);
    }
}

/// «Добавить файлы…» — диалог с мультивыбором.
fn add_files(hwnd: HWND) {
    let files = win32::open_files(hwnd, "Добавить облака точек", &OPEN_FILTER);
    if files.is_empty() {
        return;
    }
    with_state(hwnd, |state| {
        state.queue.add(&files, false);
    });
    refresh(hwnd);
}

/// «Добавить папку…» — каталог со всеми файлами (с обходом подкаталогов).
fn add_folder(hwnd: HWND) {
    let Some(folder) = win32::browse_folder(hwnd, "Выберите папку с облаками точек") else {
        return;
    };
    with_state(hwnd, |state| {
        state.queue.add(&[folder], true);
    });
    refresh(hwnd);
}

/// «Убрать» — удалить выделенный файл из очереди.
fn remove_selected(hwnd: HWND) {
    let list = win32::child(hwnd, Id::List.raw());
    let Some(index) = win32::list_selected(list) else {
        return;
    };
    with_state(hwnd, |state| {
        state.queue.remove(index);
    });
    refresh(hwnd);
}

/// Перетаскивание файлов в окно.
fn handle_drop(hwnd: HWND, hdrop: *mut c_void) {
    let files = win32::dropped_files(hdrop);
    if files.is_empty() {
        return;
    }
    with_state(hwnd, |state| {
        state.queue.add(&files, true);
    });
    refresh(hwnd);
}

/// Перерисовать список файлов и подсказку по состоянию очереди.
fn refresh(hwnd: HWND) {
    let list = win32::child(hwnd, Id::List.raw());
    win32::list_clear(list);
    let pointer = state_ptr(hwnd);
    if pointer.is_null() {
        return;
    }
    // SAFETY: указатель валиден, пока окно живо; читаем только состояние очереди.
    let state = unsafe { &*pointer };
    for (index, entry) in state.queue.entries().iter().enumerate() {
        let name = entry
            .path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
        let folder = entry
            .path
            .parent()
            .map(|value| value.display().to_string())
            .unwrap_or_default();
        let cells = vec![
            name,
            folder,
            model::format_size(entry.size),
            entry.status.label().to_string(),
        ];
        win32::list_add_row(list, index, &cells);
    }
    let hint = win32::child(hwnd, Id::Hint.raw());
    win32::set_text(hint, &model::queue_hint(&state.queue));
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
