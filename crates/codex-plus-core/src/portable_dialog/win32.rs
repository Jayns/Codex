//! Minimal native Win32 configuration dialog for portable launcher mode.
//!
//! Deliberately avoids pulling in a GUI crate or webview: a handful of
//! `EDIT`/`STATIC`/`BUTTON` child windows on a plain top-level window. Visual
//! polish (accent header band, rounded field/button outlines, flat hairline
//! separator) is done with plain GDI drawing in `WM_PAINT` / `WM_DRAWITEM`
//! rather than pulling in a theming library.

use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;

use windows::Win32::Foundation::{COLORREF, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CreateFontW, CreatePen, CreateSolidBrush, DT_SINGLELINE,
    DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FF_DONTCARE, FW_BOLD, FW_NORMAL, FillRect,
    GetDC, GetStockObject, GetTextMetricsW, HDC, HFONT, HGDIOBJ, InvalidateRect, NULL_BRUSH,
    NULL_PEN, OPAQUE, PAINTSTRUCT, PS_SOLID, ReleaseDC, RoundRect, SelectObject, SetBkColor,
    SetBkMode, SetTextColor, TEXTMETRICW, TRANSPARENT, UpdateWindow, WHITE_BRUSH,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemServices::SS_LEFT;
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;
use windows::Win32::UI::Shell::{
    DefSubclassProc, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
    SetWindowSubclass,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BN_CLICKED, BS_OWNERDRAW, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, ES_PASSWORD,
    GWLP_USERDATA, GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, KillTimer, LoadCursorW,
    MB_ICONERROR, MB_OK, MSG, MessageBoxW, PostQuitMessage, RegisterClassExW, SWP_NOMOVE,
    SWP_NOZORDER, SW_SHOW, SendMessageW, SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
    ShowWindow, TranslateMessage, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_CTLCOLOREDIT,
    WM_CTLCOLORSTATIC, WM_DESTROY, WM_DRAWITEM, WM_MOUSEMOVE, WM_PAINT, WM_SETFONT, WM_TIMER,
    WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP,
    WS_VISIBLE, WINDOW_STYLE,
};
use windows::core::PCWSTR;

use crate::portable::PortableConfig;

const CLASS_NAME: &str = "CodexPlusPortableConfigDialog";

// Accent color for the header band and primary button (a calm blue, RGB
// 0x378ADD stored as 0x00BBGGRR).
const ACCENT: u32 = 0x00DD_8A_37;
const ACCENT_HOVER: u32 = 0x00C8_7A_2C;
const ACCENT_HILITE: u32 = 0x00EE_A0_50;
const FIELD_BORDER: u32 = 0x00DD_DD_DD;
const HEADER_SUBTITLE: u32 = 0x00ED_ED_ED;
const LABEL_TEXT: u32 = 0x0040_4040;
const EDIT_TEXT: u32 = 0x0026_2626;
const SEPARATOR: u32 = 0x00E3_E3_E3;
const SECONDARY_FILL: u32 = 0x00F6_F6_F6;
const SECONDARY_FILL_HOVER: u32 = 0x00F0_F0_F0;
const SECONDARY_FILL_PRESSED: u32 = 0x00E9_E9_E9;
const SECONDARY_BORDER: u32 = 0x00D5_D5_D5;
const SECONDARY_TEXT: u32 = 0x0040_4040;

// Hover animation: a short color fade driven by a poll timer.
const HOVER_FRAMES: i32 = 6;
const ANIM_TIMER_ID: usize = 0xA11A;
const BTN_SUBCLASS_ID: usize = 1;

const PAD_X: i32 = 32;
const LABEL_X: i32 = PAD_X;
const LABEL_WIDTH: i32 = 132;
const LABEL_FIELD_GAP: i32 = 14;
const FIELD_X: i32 = LABEL_X + LABEL_WIDTH + LABEL_FIELD_GAP;
const FIELD_WIDTH: i32 = 452;
const BROWSE_WIDTH: i32 = 68;
const BROWSE_GAP: i32 = 8;
const FIELD_WIDTH_WITH_BROWSE: i32 = FIELD_WIDTH - BROWSE_WIDTH - BROWSE_GAP;
const FIELD_HEIGHT: i32 = 28;
const FIELD_BORDER_PAD: i32 = 3;
const FIELD_BORDER_RADIUS: i32 = 6;
const ROW_PITCH: i32 = 46;
const ROW_COUNT: i32 = 5;

// Header band: accent-filled band with a monogram badge, title and subtitle.
const HEADER_HEIGHT: i32 = 74;
const HEADER_BADGE_SIZE: i32 = 34;
const FORM_TOP_GAP: i32 = 26;
const FORM_TOP: i32 = HEADER_HEIGHT + FORM_TOP_GAP;
const FORM_HEIGHT: i32 = ROW_COUNT * ROW_PITCH;

const FOOTER_SEP_GAP: i32 = 16;
const FOOTER_BUTTON_GAP: i32 = 20;
const BUTTON_HEIGHT: i32 = 36;
const BUTTON_RADIUS: i32 = 8;
const BOTTOM_PAD: i32 = 24;

// Right edge shared by every field card, the browse button, the footer
// separator and the primary button, so the whole form aligns on one line.
const CARD_RIGHT: i32 = FIELD_X + FIELD_WIDTH + FIELD_BORDER_PAD;
const CLIENT_WIDTH: i32 = CARD_RIGHT + PAD_X;
const FOOTER_SEP_Y: i32 = FORM_TOP + FORM_HEIGHT + FOOTER_SEP_GAP;
const FOOTER_BUTTON_Y: i32 = FOOTER_SEP_Y + 1 + FOOTER_BUTTON_GAP;
const CLIENT_HEIGHT: i32 = FOOTER_BUTTON_Y + BUTTON_HEIGHT + BOTTOM_PAD;

const ID_EDIT_BASE_URL: i32 = 101;
const ID_EDIT_API_KEY: i32 = 102;
const ID_EDIT_MODEL: i32 = 103;
const ID_EDIT_PROVIDER: i32 = 104;
const ID_EDIT_APP_DIR: i32 = 105;
const ID_BTN_BROWSE_APP_DIR: i32 = 150;
const ID_BTN_SAVE: i32 = 201;
const ID_BTN_CANCEL: i32 = 202;

const IDX_BASE_URL: usize = 0;
const IDX_API_KEY: usize = 1;
const IDX_MODEL: usize = 2;
const IDX_PROVIDER: usize = 3;
const IDX_APP_DIR: usize = 4;

const FIELD_LABELS: [&str; 5] = [
    "API 网址",
    "API Key",
    "默认模型",
    "Provider 名称",
    "ChatGPT App 路径",
];
const FIELD_PASSWORDS: [bool; 5] = [false, true, false, false, false];
const FIELD_EDIT_IDS: [i32; 5] = [
    ID_EDIT_BASE_URL,
    ID_EDIT_API_KEY,
    ID_EDIT_MODEL,
    ID_EDIT_PROVIDER,
    ID_EDIT_APP_DIR,
];

#[derive(Clone, Copy)]
struct ButtonVisual {
    hwnd: HWND,
    id: i32,
    hovered: bool,
    anim: i32,
}

struct DialogState {
    edits: [HWND; 5],
    focused_edit: HWND,
    buttons: [ButtonVisual; 3],
    title_font: HFONT,
    subtitle_font: HFONT,
    badge_font: HFONT,
    result: Option<PortableConfig>,
    base: PortableConfig,
}

impl DialogState {
    /// Hover-fade progress (0.0 = idle, 1.0 = fully hovered) for a button id.
    fn button_hover_t(&self, id: i32) -> f32 {
        self.buttons
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.anim as f32 / HOVER_FRAMES as f32)
            .unwrap_or(0.0)
    }
}

/// Linearly blends two `0x00BBGGRR` colors (`t` clamped to 0.0..=1.0).
fn lerp_color(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |shift: u32| -> u32 {
        let ca = ((a >> shift) & 0xFF) as f32;
        let cb = ((b >> shift) & 0xFF) as f32;
        (ca + (cb - ca) * t).round() as u32 & 0xFF
    };
    mix(0) | (mix(8) << 8) | (mix(16) << 16)
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
            hbrBackground: CreateSolidBrush(COLORREF(0x00FF_FFFF)),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let title = wide_null("ChatGPT Launcher");
        let mut state = Box::new(DialogState {
            edits: [HWND::default(); 5],
            focused_edit: HWND::default(),
            buttons: [ButtonVisual {
                hwnd: HWND::default(),
                id: 0,
                hovered: false,
                anim: 0,
            }; 3],
            title_font: HFONT::default(),
            subtitle_font: HFONT::default(),
            badge_font: HFONT::default(),
            result: None,
            base: initial.clone(),
        });
        let state_ptr: *mut DialogState = state.as_mut();

        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
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

/// Returns the (x, y, width, height) of each of the five field rows, in
/// client-area pixels, shared between control layout and border painting.
fn field_rects() -> [(i32, i32, i32, i32); 5] {
    let mut rects = [(0, 0, 0, 0); 5];
    let mut y = FORM_TOP;
    for (i, rect) in rects.iter_mut().enumerate() {
        let w = if i == IDX_APP_DIR {
            FIELD_WIDTH_WITH_BROWSE
        } else {
            FIELD_WIDTH
        };
        *rect = (FIELD_X, y, w, FIELD_HEIGHT);
        y += ROW_PITCH;
    }
    rects
}

fn edit_index_for_id(id: i32) -> Option<usize> {
    FIELD_EDIT_IDS.iter().position(|&candidate| candidate == id)
}

unsafe fn create_controls(parent: HWND, instance: HMODULE, state: &mut DialogState) {
    unsafe {
        let body_font = create_font(-15, FW_NORMAL.0 as i32);
        // A single-line EDIT draws its text at the top of its client area, so
        // size the control to ~one line and center that box within the field
        // card; otherwise the text sits visibly high in a tall control.
        let edit_h = (font_line_height(body_font) + 2).clamp(18, FIELD_HEIGHT - 6);
        state.title_font = create_font(-22, FW_BOLD.0 as i32);
        state.subtitle_font = create_font(-14, FW_NORMAL.0 as i32);
        state.badge_font = create_font(-18, FW_BOLD.0 as i32);

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

        let make_label = |text: &str, y: i32| {
            // Left-aligned label, vertically centered against its field row.
            make_static(text, LABEL_X, y + 5, LABEL_WIDTH, 20, label_style, body_font);
        };

        let make_edit = |id: i32, y: i32, width: i32, text: &str, password: bool| -> HWND {
            let text = wide_null(text);
            let style = WS_CHILD
                | WS_VISIBLE
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
                edit_h,
                parent,
                HMENU(id as *mut std::ffi::c_void),
                instance,
                None,
            )
            .unwrap_or_default();
            set_font(hwnd, body_font);
            hwnd
        };

        // All buttons are owner-drawn (rounded, flat) via WM_DRAWITEM.
        let make_button = |id: i32, x: i32, y: i32, w: i32, h: i32, text: &str| -> HWND {
            let text = wide_null(text);
            let style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32);
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

        let values = [
            state.base.api_base_url.clone(),
            state.base.api_key.clone(),
            state.base.model.clone(),
            state.base.provider_name.clone(),
            state.base.codex_app_dir.clone(),
        ];
        let rects = field_rects();
        for i in 0..5 {
            let (_x, y, w, _h) = rects[i];
            make_label(FIELD_LABELS[i], y);
            let edit_y = y + (FIELD_HEIGHT - edit_h) / 2;
            state.edits[i] = make_edit(FIELD_EDIT_IDS[i], edit_y, w, &values[i], FIELD_PASSWORDS[i]);
        }

        // Browse button: its card spans from just right of the shortened
        // app-dir card to the shared `CARD_RIGHT`, matching the field height.
        let (_app_dir_x, app_dir_y, app_dir_w, _h) = rects[IDX_APP_DIR];
        let browse_x = FIELD_X + app_dir_w + FIELD_BORDER_PAD + BROWSE_GAP;
        let browse = make_button(
            ID_BTN_BROWSE_APP_DIR,
            browse_x,
            app_dir_y - FIELD_BORDER_PAD,
            CARD_RIGHT - browse_x,
            FIELD_HEIGHT + FIELD_BORDER_PAD * 2,
            "浏览",
        );

        // Footer: hairline separator (hand-drawn in WM_PAINT), then secondary
        // + accent primary buttons, right edge aligned to `CARD_RIGHT`.
        let save_width = 168;
        let cancel_width = 96;
        let button_gap = 10;
        let save_x = CARD_RIGHT - save_width;
        let cancel_x = save_x - button_gap - cancel_width;
        let cancel = make_button(
            ID_BTN_CANCEL,
            cancel_x,
            FOOTER_BUTTON_Y,
            cancel_width,
            BUTTON_HEIGHT,
            "退出",
        );
        let save = make_button(
            ID_BTN_SAVE,
            save_x,
            FOOTER_BUTTON_Y,
            save_width,
            BUTTON_HEIGHT,
            "保存并启动 ChatGPT",
        );

        state.buttons = [
            ButtonVisual { hwnd: save, id: ID_BTN_SAVE, hovered: false, anim: 0 },
            ButtonVisual { hwnd: cancel, id: ID_BTN_CANCEL, hovered: false, anim: 0 },
            ButtonVisual { hwnd: browse, id: ID_BTN_BROWSE_APP_DIR, hovered: false, anim: 0 },
        ];
        let state_ref = state as *mut DialogState as usize;
        for button in &state.buttons {
            let _ = SetWindowSubclass(
                button.hwnd,
                Some(button_subclass_proc),
                BTN_SUBCLASS_ID,
                state_ref,
            );
        }
    }
}

/// Button subclass: detects the pointer entering an owner-drawn button and
/// starts the shared hover-fade timer on the parent. Leaving is handled by
/// the timer's own cursor hit-test (avoids needing `TrackMouseEvent`).
unsafe extern "system" fn button_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    unsafe {
        if msg == WM_MOUSEMOVE {
            let state = refdata as *mut DialogState;
            if !state.is_null() {
                let ctl_id = window_ctl_id(hwnd);
                if let Some(slot) = (*state).buttons.iter_mut().find(|b| b.id == ctl_id) {
                    if !slot.hovered {
                        slot.hovered = true;
                        let parent = GetParentSafe(hwnd);
                        SetTimer(parent, ANIM_TIMER_ID, 16, None);
                        let _ = InvalidateRect(hwnd, None, true);
                    }
                }
            }
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

#[allow(non_snake_case)]
unsafe fn GetParentSafe(hwnd: HWND) -> HWND {
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetParent(hwnd).unwrap_or_default() }
}

fn window_ctl_id(hwnd: HWND) -> i32 {
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetDlgCtrlID(hwnd) }
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

/// Total pixel height of one text line in `font`, used to size single-line
/// EDIT controls so their text lands vertically centered in the field card
/// (an over-tall single-line edit renders its text near the top instead).
unsafe fn font_line_height(font: HFONT) -> i32 {
    unsafe {
        let hdc = GetDC(HWND::default());
        let previous = SelectObject(hdc, font);
        let mut tm = TEXTMETRICW::default();
        let _ = GetTextMetricsW(hdc, &mut tm);
        SelectObject(hdc, previous);
        let _ = ReleaseDC(HWND::default(), hdc);
        tm.tmHeight + tm.tmExternalLeading
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const DialogState;
                let state = if state_ptr.is_null() { None } else { Some(&*state_ptr) };
                paint_header(hdc, state);
                paint_field_borders(hdc, state);
                paint_footer_separator(hdc);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_CTLCOLORSTATIC => {
                let hdc = HDC(wparam.0 as *mut std::ffi::c_void);
                let _ = SetBkMode(hdc, TRANSPARENT);
                SetTextColor(hdc, COLORREF(LABEL_TEXT));
                LRESULT(GetStockObject(WHITE_BRUSH).0 as isize)
            }
            WM_CTLCOLOREDIT => {
                let hdc = HDC(wparam.0 as *mut std::ffi::c_void);
                let _ = SetBkMode(hdc, OPAQUE);
                let _ = SetBkColor(hdc, COLORREF(0x00FF_FFFF));
                SetTextColor(hdc, COLORREF(EDIT_TEXT));
                LRESULT(GetStockObject(WHITE_BRUSH).0 as isize)
            }
            WM_DRAWITEM => {
                let dis = lparam.0 as *const DRAWITEMSTRUCT;
                if !dis.is_null() {
                    let ctl_id = (*dis).CtlID as i32;
                    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const DialogState;
                    let hover_t = if state_ptr.is_null() {
                        0.0
                    } else {
                        (*state_ptr).button_hover_t(ctl_id)
                    };
                    if ctl_id == ID_BTN_SAVE {
                        draw_button(&*dis, true, hover_t);
                        return LRESULT(1);
                    } else if ctl_id == ID_BTN_CANCEL || ctl_id == ID_BTN_BROWSE_APP_DIR {
                        draw_button(&*dis, false, hover_t);
                        return LRESULT(1);
                    }
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == ANIM_TIMER_ID => {
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState;
                if !state_ptr.is_null() {
                    tick_button_hover(hwnd, &mut *state_ptr);
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
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState;
                if state_ptr.is_null() {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                }
                let state = &mut *state_ptr;

                const EN_SETFOCUS: u32 = 0x0100;
                const EN_KILLFOCUS: u32 = 0x0200;
                if notification == EN_SETFOCUS || notification == EN_KILLFOCUS {
                    if let Some(idx) = edit_index_for_id(control_id) {
                        state.focused_edit = if notification == EN_SETFOCUS {
                            state.edits[idx]
                        } else {
                            HWND::default()
                        };
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                    return LRESULT(0);
                }

                if notification != BN_CLICKED as u32 {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                }
                match control_id {
                    ID_BTN_BROWSE_APP_DIR => {
                        if let Some(path) = browse_for_folder(hwnd) {
                            let wide = wide_null(&path);
                            let _ = SetWindowTextW(state.edits[IDX_APP_DIR], PCWSTR(wide.as_ptr()));
                        }
                    }
                    ID_BTN_SAVE => {
                        state.result = Some(PortableConfig {
                            api_base_url: read_edit_text(state.edits[IDX_BASE_URL]),
                            api_key: read_edit_text(state.edits[IDX_API_KEY]),
                            model: read_edit_text(state.edits[IDX_MODEL]),
                            provider_name: read_edit_text(state.edits[IDX_PROVIDER]),
                            codex_app_dir: read_edit_text(state.edits[IDX_APP_DIR]),
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

/// Paints the accent header band: a solid color fill, a small monogram
/// badge, the app title and a muted subtitle line.
unsafe fn paint_header(hdc: HDC, state: Option<&DialogState>) {
    unsafe {
        let header_rc = RECT {
            left: 0,
            top: 0,
            right: CLIENT_WIDTH,
            bottom: HEADER_HEIGHT,
        };
        let brush = CreateSolidBrush(COLORREF(ACCENT));
        FillRect(hdc, &header_rc, brush);
        let _ = DeleteObject(HGDIOBJ::from(brush));

        let badge_x = PAD_X;
        let badge_y = (HEADER_HEIGHT - HEADER_BADGE_SIZE) / 2;
        let mut badge_rc = RECT {
            left: badge_x,
            top: badge_y,
            right: badge_x + HEADER_BADGE_SIZE,
            bottom: badge_y + HEADER_BADGE_SIZE,
        };
        let badge_brush = CreateSolidBrush(COLORREF(0x00FF_FFFF));
        let old_brush = SelectObject(hdc, badge_brush);
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        let _ = RoundRect(
            hdc,
            badge_rc.left,
            badge_rc.top,
            badge_rc.right,
            badge_rc.bottom,
            10,
            10,
        );
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        let _ = DeleteObject(HGDIOBJ::from(badge_brush));

        let Some(state) = state else { return };

        let old_font = SelectObject(hdc, state.badge_font);
        let _ = SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(ACCENT));
        let mut badge_text = wide_text("C");
        let _ = DrawTextW(hdc, &mut badge_text, &mut badge_rc, DT_SINGLELINE | DT_VCENTER | dt_center());

        let title_x = badge_x + HEADER_BADGE_SIZE + 14;
        SelectObject(hdc, state.title_font);
        SetTextColor(hdc, COLORREF(0x00FF_FFFF));
        let mut title_rc = RECT {
            left: title_x,
            top: 14,
            right: CLIENT_WIDTH - PAD_X,
            bottom: 14 + 26,
        };
        let mut title_text = wide_text("ChatGPT Launcher");
        let _ = DrawTextW(hdc, &mut title_text, &mut title_rc, DT_SINGLELINE | DT_VCENTER);

        SelectObject(hdc, state.subtitle_font);
        SetTextColor(hdc, COLORREF(HEADER_SUBTITLE));
        let mut subtitle_rc = RECT {
            left: title_x,
            top: 42,
            right: CLIENT_WIDTH - PAD_X,
            bottom: 42 + 20,
        };
        let mut subtitle_text = wide_text("填写 API 信息，保存后自动启动 ChatGPT");
        let _ = DrawTextW(hdc, &mut subtitle_text, &mut subtitle_rc, DT_SINGLELINE | DT_VCENTER);

        SelectObject(hdc, old_font);
    }
}

/// Draws a rounded outline around each field row; the currently focused
/// field is highlighted with the accent color instead of neutral gray.
unsafe fn paint_field_borders(hdc: HDC, state: Option<&DialogState>) {
    unsafe {
        for (i, &(x, y, w, h)) in field_rects().iter().enumerate() {
            let rc = RECT {
                left: x - FIELD_BORDER_PAD,
                top: y - FIELD_BORDER_PAD,
                right: x + w + FIELD_BORDER_PAD,
                bottom: y + h + FIELD_BORDER_PAD,
            };
            let is_focused = state.is_some_and(|s| {
                s.focused_edit != HWND::default() && s.edits[i] == s.focused_edit
            });
            let color = if is_focused { ACCENT } else { FIELD_BORDER };
            draw_rounded_outline(hdc, rc, FIELD_BORDER_RADIUS, color);
        }
    }
}

unsafe fn paint_footer_separator(hdc: HDC) {
    unsafe {
        let rc = RECT {
            left: PAD_X,
            top: FOOTER_SEP_Y,
            right: CARD_RIGHT,
            bottom: FOOTER_SEP_Y + 1,
        };
        let brush = CreateSolidBrush(COLORREF(SEPARATOR));
        FillRect(hdc, &rc, brush);
        let _ = DeleteObject(HGDIOBJ::from(brush));
    }
}

/// Draws a hollow rounded rectangle outline (no fill) in `color`.
unsafe fn draw_rounded_outline(hdc: HDC, rc: RECT, radius: i32, color: u32) {
    unsafe {
        let pen = CreatePen(PS_SOLID, 1, COLORREF(color));
        let old_pen = SelectObject(hdc, pen);
        let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
        let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, radius, radius);
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
        let _ = DeleteObject(HGDIOBJ::from(pen));
    }
}

/// Paints one owner-drawn button: solid accent fill for the primary action,
/// a light bordered card for secondary actions, both with rounded corners.
/// `hover_t` (0.0..=1.0) fades in a brighter tint as the pointer hovers.
unsafe fn draw_button(dis: &DRAWITEMSTRUCT, primary: bool, hover_t: f32) {
    unsafe {
        const ODS_SELECTED: u32 = 0x0001;
        let hdc = dis.hDC;
        let pressed = dis.itemState.0 & ODS_SELECTED != 0;
        let (fill, border, text_color) = if primary {
            let base = if pressed {
                ACCENT_HOVER
            } else {
                lerp_color(ACCENT, ACCENT_HILITE, hover_t)
            };
            (base, base, 0x00FF_FFFFu32)
        } else if pressed {
            (SECONDARY_FILL_PRESSED, SECONDARY_BORDER, SECONDARY_TEXT)
        } else {
            let fill = lerp_color(SECONDARY_FILL, SECONDARY_FILL_HOVER, hover_t);
            let border = lerp_color(SECONDARY_BORDER, ACCENT, hover_t * 0.55);
            (fill, border, SECONDARY_TEXT)
        };

        let rc = dis.rcItem;
        let brush = CreateSolidBrush(COLORREF(fill));
        let pen = CreatePen(PS_SOLID, 1, COLORREF(border));
        let old_brush = SelectObject(hdc, brush);
        let old_pen = SelectObject(hdc, pen);
        let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, BUTTON_RADIUS, BUTTON_RADIUS);
        SelectObject(hdc, old_brush);
        SelectObject(hdc, old_pen);
        let _ = DeleteObject(HGDIOBJ::from(brush));
        let _ = DeleteObject(HGDIOBJ::from(pen));

        let _ = SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(text_color));
        let mut text = button_text(dis.hwndItem);
        let mut rc_mut = rc;
        let _ = DrawTextW(hdc, &mut text, &mut rc_mut, DT_CENTER_VCENTER());
    }
}

/// Poll-driven hover animation step: hit-test the cursor against each button,
/// advance its fade toward the hovered/idle target and repaint. Stops the
/// shared timer once every button is idle again.
unsafe fn tick_button_hover(hwnd: HWND, state: &mut DialogState) {
    unsafe {
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let mut still_active = false;
        for slot in state.buttons.iter_mut() {
            if slot.hwnd == HWND::default() {
                continue;
            }
            let mut rc = RECT::default();
            let over = GetWindowRect(slot.hwnd, &mut rc).is_ok()
                && cursor.x >= rc.left
                && cursor.x < rc.right
                && cursor.y >= rc.top
                && cursor.y < rc.bottom;
            slot.hovered = over;
            let target = if over { HOVER_FRAMES } else { 0 };
            if slot.anim != target {
                slot.anim += (target - slot.anim).signum();
                let _ = InvalidateRect(slot.hwnd, None, true);
            }
            if slot.hovered || slot.anim != 0 {
                still_active = true;
            }
        }
        if !still_active {
            let _ = KillTimer(hwnd, ANIM_TIMER_ID);
        }
    }
}

#[allow(non_snake_case)]
fn DT_CENTER_VCENTER() -> windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT {
    dt_center() | DT_VCENTER | DT_SINGLELINE
}

fn dt_center() -> windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT {
    windows::Win32::Graphics::Gdi::DT_CENTER
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

/// Same as `wide_null` but without a trailing NUL, for `DrawTextW` calls
/// that take an explicit-length slice rather than a C string.
fn wide_text(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().collect()
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

    #[test]
    fn field_rects_are_within_client_bounds() {
        for (x, y, w, h) in field_rects() {
            assert!(x >= 0 && y >= 0);
            assert!(x + w <= CLIENT_WIDTH);
            assert!(y + h <= CLIENT_HEIGHT);
        }
    }
}
