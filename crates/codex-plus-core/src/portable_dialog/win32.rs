//! Minimal native Win32 configuration dialog for portable launcher mode.
//!
//! Deliberately avoids pulling in a GUI crate or webview: a handful of
//! `EDIT`/`STATIC`/`BUTTON` child windows on a plain top-level window, laid out
//! with a header band, left-aligned labels, an accent (owner-drawn) primary
//! button, and hairline separators to feel coordinated without leaving native
//! GDI.

use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;

use windows::Win32::Foundation::{COLORREF, HMODULE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, COLOR_BTNFACE, CreateFontW, CreateSolidBrush, DT_SINGLELINE, DT_VCENTER,
    DrawTextW, FF_DONTCARE, FW_NORMAL, FillRect, GetSysColorBrush, HDC, HFONT, SetBkMode,
    SetTextColor, TRANSPARENT, UpdateWindow,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemServices::{SS_ETCHEDHORZ, SS_LEFT};
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;
use windows::Win32::UI::Shell::{
    FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BN_CLICKED, BS_OWNERDRAW, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, ES_PASSWORD,
    GWLP_USERDATA, GetClientRect, GetMessageW, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, LoadCursorW, MB_ICONERROR, MB_OK,
    MSG, MessageBoxW, PostQuitMessage,
    RegisterClassExW, SWP_NOMOVE, SWP_NOZORDER, SW_SHOW, SendMessageW, SetWindowLongPtrW,
    SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, WM_CLOSE, WM_COMMAND, WM_CREATE,
    WM_CTLCOLORSTATIC, WM_DESTROY, WM_DRAWITEM, WM_SETFONT, WNDCLASSEXW, WS_BORDER, WS_CAPTION,
    WS_CHILD, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WINDOW_STYLE,
};
use windows::core::PCWSTR;

use crate::portable::PortableConfig;

const CLASS_NAME: &str = "CodexPlusPortableConfigDialog";

// Accent color for the primary button (a calm blue, RGB 0x378ADD as 0x00BBGGRR).
const ACCENT: u32 = 0x00DD_8A_37;
const ACCENT_HOVER: u32 = 0x00C8_7A_2C;

const PAD_X: i32 = 28;
const LABEL_X: i32 = PAD_X;
const LABEL_WIDTH: i32 = 132;
const LABEL_FIELD_GAP: i32 = 14;
const FIELD_X: i32 = LABEL_X + LABEL_WIDTH + LABEL_FIELD_GAP;
const FIELD_WIDTH: i32 = 452;
const BROWSE_WIDTH: i32 = 64;
const BROWSE_GAP: i32 = 8;
const FIELD_WIDTH_WITH_BROWSE: i32 = FIELD_WIDTH - BROWSE_WIDTH - BROWSE_GAP;
const FIELD_HEIGHT: i32 = 28;
const ROW_PITCH: i32 = 44;
const ROW_COUNT: i32 = 5;

// Vertical rhythm (all in client-area pixels).
const HEADER_TOP: i32 = 18;
const HEADER_SUBTITLE_H: i32 = 18;
const HEADER_BOTTOM_PAD: i32 = 16;
const FORM_TOP: i32 = HEADER_TOP + HEADER_SUBTITLE_H + HEADER_BOTTOM_PAD + 1;
const FORM_HEIGHT: i32 = ROW_COUNT * ROW_PITCH;
const FOOTER_TOP_PAD: i32 = 14;
const BUTTON_HEIGHT: i32 = 34;
const BOTTOM_PAD: i32 = 20;

const CLIENT_WIDTH: i32 = FIELD_X + FIELD_WIDTH + PAD_X;
const CLIENT_HEIGHT: i32 = FORM_TOP + FORM_HEIGHT + FOOTER_TOP_PAD + 1 + 16 + BUTTON_HEIGHT + BOTTOM_PAD;

const ID_EDIT_BASE_URL: i32 = 101;
const ID_EDIT_API_KEY: i32 = 102;
const ID_EDIT_MODEL: i32 = 103;
const ID_EDIT_PROVIDER: i32 = 104;
const ID_EDIT_APP_DIR: i32 = 105;
const ID_BTN_BROWSE_APP_DIR: i32 = 150;
const ID_BTN_SAVE: i32 = 201;
const ID_BTN_CANCEL: i32 = 202;

struct DialogState {
    edit_base_url: HWND,
    edit_api_key: HWND,
    edit_model: HWND,
    edit_provider: HWND,
    edit_app_dir: HWND,
    subtitle: HWND,
    result: Option<PortableConfig>,
    base: PortableConfig,
}

/// Shows the configuration window and blocks until the user saves or closes it.
///
/// Returns `Some(config)` when the user clicked "保存并启动 Codex", or `None`
/// when the window was closed/cancelled (caller should not launch Codex).
pub fn show_portable_config_dialog(
    initial: &PortableConfig,
) -> anyhow::Result<Option<PortableConfig>> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let instance: HMODULE = GetModuleHandleW(None)?;
        let class_name = wide_null(CLASS_NAME);

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let title = wide_null("ChatGPT Launcher");
        let mut state = Box::new(DialogState {
            edit_base_url: HWND::default(),
            edit_api_key: HWND::default(),
            edit_model: HWND::default(),
            edit_provider: HWND::default(),
            edit_app_dir: HWND::default(),
            subtitle: HWND::default(),
            result: None,
            base: initial.clone(),
        });
        let state_ptr: *mut DialogState = state.as_mut();

        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        let hwnd = CreateWindowExW(
            Default::default(),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CLIENT_WIDTH,
            CLIENT_HEIGHT,
            HWND::default(),
            HMENU::default(),
            instance,
            Some(state_ptr as *const std::ffi::c_void),
        )?;

        fit_client_area(hwnd, CLIENT_WIDTH, CLIENT_HEIGHT);
        create_controls(hwnd, instance, &mut state);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        CoUninitialize();
        Ok(state.result.take())
    }
}

unsafe fn create_controls(parent: HWND, instance: HMODULE, state: &mut DialogState) {
    unsafe {
        let body_font = create_font(-15, FW_NORMAL.0 as i32);

        let label_class = wide_null("STATIC");
        let edit_class = wide_null("EDIT");
        let button_class = wide_null("BUTTON");

        let set_font = |hwnd: HWND, font: HFONT| {
            let _ = SendMessageW(hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        };

        let make_static = |text: &str, x: i32, y: i32, w: i32, h: i32, style: WINDOW_STYLE, font: HFONT| -> HWND {
            let text = wide_null(text);
            let hwnd = CreateWindowExW(
                Default::default(),
                PCWSTR(label_class.as_ptr()),
                PCWSTR(text.as_ptr()),
                WS_CHILD | WS_VISIBLE | style,
                x,
                y,
                w,
                h,
                parent,
                HMENU::default(),
                instance,
                None,
            )
            .unwrap_or_default();
            set_font(hwnd, font);
            hwnd
        };

        let label_style = WINDOW_STYLE(SS_LEFT.0);
        let content_w = CLIENT_WIDTH - PAD_X * 2;

        // Header: a single descriptive line, then a separator hairline.
        state.subtitle = make_static(
            "填写 API 信息，保存后自动启动 ChatGPT",
            PAD_X,
            HEADER_TOP,
            content_w,
            HEADER_SUBTITLE_H,
            label_style,
            body_font,
        );
        let sep_y = FORM_TOP - HEADER_BOTTOM_PAD / 2 - 1;
        make_static(
            "",
            PAD_X,
            sep_y,
            content_w,
            2,
            WINDOW_STYLE(SS_ETCHEDHORZ.0),
            body_font,
        );

        let make_label = |text: &str, y: i32| {
            // Left-aligned label, vertically centered against its field row.
            make_static(text, LABEL_X, y + 5, LABEL_WIDTH, 20, label_style, body_font);
        };

        let make_edit = |id: i32, y: i32, width: i32, text: &str, password: bool| -> HWND {
            let text = wide_null(text);
            let style = WS_CHILD
                | WS_VISIBLE
                | WS_BORDER
                | WS_TABSTOP
                | WINDOW_STYLE(ES_AUTOHSCROLL as u32)
                | if password {
                    WINDOW_STYLE(ES_PASSWORD as u32)
                } else {
                    WINDOW_STYLE(0)
                };
            let hwnd = CreateWindowExW(
                Default::default(),
                PCWSTR(edit_class.as_ptr()),
                PCWSTR(text.as_ptr()),
                style,
                FIELD_X,
                y,
                width,
                FIELD_HEIGHT,
                parent,
                HMENU(id as *mut std::ffi::c_void),
                instance,
                None,
            )
            .unwrap_or_default();
            set_font(hwnd, body_font);
            hwnd
        };

        let make_button = |id: i32, x: i32, y: i32, w: i32, h: i32, text: &str, owner_draw: bool| -> HWND {
            let text = wide_null(text);
            let mut style = WS_CHILD | WS_VISIBLE | WS_TABSTOP;
            if owner_draw {
                style |= WINDOW_STYLE(BS_OWNERDRAW as u32);
            }
            let hwnd = CreateWindowExW(
                Default::default(),
                PCWSTR(button_class.as_ptr()),
                PCWSTR(text.as_ptr()),
                style,
                x,
                y,
                w,
                h,
                parent,
                HMENU(id as *mut std::ffi::c_void),
                instance,
                None,
            )
            .unwrap_or_default();
            set_font(hwnd, body_font);
            hwnd
        };

        let mut y = FORM_TOP;
        make_label("API 网址", y);
        state.edit_base_url = make_edit(ID_EDIT_BASE_URL, y, FIELD_WIDTH, &state.base.api_base_url, false);
        y += ROW_PITCH;

        make_label("API Key", y);
        state.edit_api_key = make_edit(ID_EDIT_API_KEY, y, FIELD_WIDTH, &state.base.api_key, true);
        y += ROW_PITCH;

        make_label("默认模型", y);
        state.edit_model = make_edit(ID_EDIT_MODEL, y, FIELD_WIDTH, &state.base.model, false);
        y += ROW_PITCH;

        make_label("Provider 名称", y);
        state.edit_provider = make_edit(ID_EDIT_PROVIDER, y, FIELD_WIDTH, &state.base.provider_name, false);
        y += ROW_PITCH;

        make_label("ChatGPT App 路径", y);
        state.edit_app_dir = make_edit(
            ID_EDIT_APP_DIR,
            y,
            FIELD_WIDTH_WITH_BROWSE,
            &state.base.codex_app_dir,
            false,
        );
        make_button(
            ID_BTN_BROWSE_APP_DIR,
            FIELD_X + FIELD_WIDTH_WITH_BROWSE + BROWSE_GAP,
            y,
            BROWSE_WIDTH,
            FIELD_HEIGHT,
            "浏览",
            false,
        );

        // Footer: separator hairline, then secondary + accent primary buttons
        // aligned to the right edge of the field column.
        let footer_sep_y = FORM_TOP + FORM_HEIGHT + FOOTER_TOP_PAD / 2;
        make_static(
            "",
            PAD_X,
            footer_sep_y,
            content_w,
            2,
            WINDOW_STYLE(SS_ETCHEDHORZ.0),
            body_font,
        );

        let footer_y = FORM_TOP + FORM_HEIGHT + FOOTER_TOP_PAD + 1 + 16;
        let save_width = 168;
        let cancel_width = 96;
        let button_gap = 10;
        let field_right = FIELD_X + FIELD_WIDTH;
        let save_x = field_right - save_width;
        let cancel_x = save_x - button_gap - cancel_width;
        make_button(ID_BTN_CANCEL, cancel_x, footer_y, cancel_width, BUTTON_HEIGHT, "退出", false);
        make_button(ID_BTN_SAVE, save_x, footer_y, save_width, BUTTON_HEIGHT, "保存并启动 ChatGPT", true);
    }
}

unsafe fn create_font(height: i32, weight: i32) -> HFONT {
    unsafe {
        let face = wide_null("Microsoft YaHei UI");
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            0,
            0,
            0,
            CLEARTYPE_QUALITY.0 as u32,
            FF_DONTCARE.0 as u32,
            PCWSTR(face.as_ptr()),
        )
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CTLCOLORSTATIC => {
                let hdc = HDC(wparam.0 as *mut std::ffi::c_void);
                let _ = SetBkMode(hdc, TRANSPARENT);
                // Subtitle in muted gray, everything else default text color.
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const DialogState;
                if !state_ptr.is_null() && lparam.0 == (*state_ptr).subtitle.0 as isize {
                    SetTextColor(hdc, COLORREF(0x00707070));
                }
                LRESULT(GetSysColorBrush(COLOR_BTNFACE).0 as isize)
            }
            WM_DRAWITEM => {
                let dis = lparam.0 as *const DRAWITEMSTRUCT;
                if !dis.is_null() && (*dis).CtlID == ID_BTN_SAVE as u32 {
                    draw_accent_button(&*dis);
                    return LRESULT(1);
                }
                LRESULT(0)
            }
            WM_CREATE => {
                let create_struct = lparam.0 as *const CREATESTRUCTW;
                if !create_struct.is_null() {
                    let state_ptr = (*create_struct).lpCreateParams as isize;
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr);
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                let control_id = (wparam.0 & 0xffff) as i32;
                let notification = ((wparam.0 >> 16) & 0xffff) as u32;
                if notification != BN_CLICKED as u32 {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                }
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState;
                if state_ptr.is_null() {
                    return LRESULT(0);
                }
                let state = &mut *state_ptr;
                match control_id {
                    ID_BTN_BROWSE_APP_DIR => {
                        if let Some(path) = browse_for_folder(hwnd) {
                            let wide = wide_null(&path);
                            let _ = SetWindowTextW(state.edit_app_dir, PCWSTR(wide.as_ptr()));
                        }
                    }
                    ID_BTN_SAVE => {
                        state.result = Some(PortableConfig {
                            api_base_url: read_edit_text(state.edit_base_url),
                            api_key: read_edit_text(state.edit_api_key),
                            model: read_edit_text(state.edit_model),
                            provider_name: read_edit_text(state.edit_provider),
                            codex_app_dir: read_edit_text(state.edit_app_dir),
                            debug_port: state.base.debug_port,
                            last_synced_hash: state.base.last_synced_hash.clone(),
                        });
                        let _ = DestroyWindow(hwnd);
                    }
                    ID_BTN_CANCEL => {
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Paints the accent (primary) button: solid accent fill, white centered text,
/// slightly darker when pressed.
unsafe fn draw_accent_button(dis: &DRAWITEMSTRUCT) {
    unsafe {
        const ODS_SELECTED: u32 = 0x0001;
        let hdc = dis.hDC;
        let pressed = dis.itemState.0 & ODS_SELECTED != 0;
        let fill = if pressed { ACCENT_HOVER } else { ACCENT };
        let brush = CreateSolidBrush(COLORREF(fill));
        let mut rc = dis.rcItem;
        FillRect(hdc, &rc, brush);
        let gdi_obj: windows::Win32::Graphics::Gdi::HGDIOBJ = brush.into();
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(gdi_obj);

        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(0x00FFFFFF));
        let mut text = button_text(dis.hwndItem);
        let _ = DrawTextW(hdc, &mut text, &mut rc, DT_CENTER_VCENTER());
    }
}

#[allow(non_snake_case)]
fn DT_CENTER_VCENTER() -> windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT {
    use windows::Win32::Graphics::Gdi::DT_CENTER;
    DT_CENTER | DT_VCENTER | DT_SINGLELINE
}

fn button_text(hwnd: HWND) -> Vec<u16> {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        let mut buffer = vec![0u16; (len.max(0) + 1) as usize];
        let copied = GetWindowTextW(hwnd, &mut buffer);
        buffer.truncate(copied as usize);
        buffer
    }
}

/// Resizes `hwnd` so its client area is exactly `client_w` × `client_h`, by
/// measuring the real frame thickness. DPI-proof, unlike AdjustWindowRect.
fn fit_client_area(hwnd: HWND, client_w: i32, client_h: i32) {
    unsafe {
        let mut client = RECT::default();
        let mut window = RECT::default();
        if GetClientRect(hwnd, &mut client).is_err() || GetWindowRect(hwnd, &mut window).is_err() {
            return;
        }
        let frame_w = (window.right - window.left) - (client.right - client.left);
        let frame_h = (window.bottom - window.top) - (client.bottom - client.top);
        let _ = SetWindowPos(
            hwnd,
            HWND::default(),
            0,
            0,
            client_w + frame_w,
            client_h + frame_h,
            SWP_NOMOVE | SWP_NOZORDER,
        );
    }
}

/// Opens the modern Windows folder-picker and returns the chosen path.
fn browse_for_folder(owner: HWND) -> Option<String> {
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog.SetOptions(options | FOS_PICKFOLDERS).ok()?;
        dialog.Show(owner).ok()?;
        let item = dialog.GetResult().ok()?;
        let display_name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = display_name.to_string().ok()?;
        CoTaskMemFree(Some(display_name.0 as *const std::ffi::c_void));
        Some(path)
    }
}

fn read_edit_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; (len + 1) as usize];
        let copied = GetWindowTextW(hwnd, &mut buffer);
        String::from_utf16_lossy(&buffer[..copied as usize])
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(once(0)).collect()
}

/// Shows a blocking native error dialog. The portable launcher is built with
/// `windows_subsystem = "windows"` (no console), so a startup failure written
/// to stderr is invisible; this is the only way the user learns what failed.
pub fn show_portable_error_dialog(message: &str) {
    let title = wide_null("ChatGPT Launcher");
    let text = wide_null(message);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_null_terminates_with_zero() {
        let wide = wide_null("abc");
        assert_eq!(wide, vec!['a' as u16, 'b' as u16, 'c' as u16, 0]);
    }

    #[test]
    fn client_dimensions_are_positive() {
        assert!(CLIENT_WIDTH > 0 && CLIENT_HEIGHT > 0);
    }
}
