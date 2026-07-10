//! Native macOS (Cocoa/AppKit) configuration dialog for the portable
//! launcher. Mirrors the behaviour of the Windows dialog (`win32.rs`): a
//! modal window with five text fields (prefilled from `initial`), a "浏览"
//! button next to the Codex App path field that opens an `NSOpenPanel`, and
//! two buttons ("退出" / "保存并启动 Codex") that resolve the call.

use std::cell::{Cell, OnceCell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSModalResponse,
    NSModalResponseCancel, NSModalResponseOK, NSOpenPanel, NSSecureTextField, NSTextField, NSView,
    NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    ns_string, MainThreadMarker, NSDate, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect,
    NSRunLoop, NSSize, NSString, NSURL,
};

use crate::portable::PortableConfig;

const PAD_X: f64 = 24.0;
const LABEL_WIDTH: f64 = 116.0;
const LABEL_FIELD_GAP: f64 = 12.0;
const FIELD_X: f64 = PAD_X + LABEL_WIDTH + LABEL_FIELD_GAP;
const CONTENT_WIDTH: f64 = 560.0;
const FIELD_WIDTH: f64 = CONTENT_WIDTH - FIELD_X - PAD_X;
const BROWSE_WIDTH: f64 = 64.0;
const BROWSE_GAP: f64 = 8.0;
const FIELD_WIDTH_WITH_BROWSE: f64 = FIELD_WIDTH - BROWSE_WIDTH - BROWSE_GAP;
const ROW_HEIGHT: f64 = 24.0;
const ROW_PITCH: f64 = 40.0;
const ROW_COUNT: f64 = 5.0;
const HEADER_HEIGHT: f64 = 56.0;
const FORM_HEIGHT: f64 = ROW_COUNT * ROW_PITCH;
const FOOTER_HEIGHT: f64 = 70.0;
const CONTENT_HEIGHT: f64 = HEADER_HEIGHT + FORM_HEIGHT + FOOTER_HEIGHT;
const BUTTON_HEIGHT: f64 = 32.0;

/// Per-window mutable state, owned by the delegate object that also serves as
/// the AppKit button target and window delegate.
struct DialogIvars {
    base: PortableConfig,
    edit_base_url: OnceCell<Retained<NSTextField>>,
    edit_api_key: OnceCell<Retained<NSSecureTextField>>,
    edit_model: OnceCell<Retained<NSTextField>>,
    edit_provider: OnceCell<Retained<NSTextField>>,
    edit_app_dir: OnceCell<Retained<NSTextField>>,
    // Set the first time `finish` runs. Guards against `windowWillClose:`
    // firing a second time when `window.close()` is called explicitly after
    // a Save/Cancel button already resolved the dialog (which would
    // otherwise clobber the just-saved result with `None`).
    resolved: Cell<bool>,
    result: RefCell<Option<PortableConfig>>,
}

define_class!(
    // SAFETY:
    // - The superclass NSObject does not have any subclassing requirements.
    // - `ConfigDialogDelegate` does not implement `Drop`.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = DialogIvars]
    struct ConfigDialogDelegate;

    // SAFETY: `NSObjectProtocol` has no safety requirements.
    unsafe impl NSObjectProtocol for ConfigDialogDelegate {}

    // SAFETY: `NSWindowDelegate` has no safety requirements.
    unsafe impl NSWindowDelegate for ConfigDialogDelegate {
        // SAFETY: The signature is correct.
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            // The user closed the window via the titlebar button without
            // saving: treat like "退出".
            self.finish(NSModalResponseCancel, None);
        }
    }

    impl ConfigDialogDelegate {
        // SAFETY: All three are plain `-(void)action:(id)sender` selectors.
        #[unsafe(method(browseAppDir:))]
        fn browse_app_dir(&self, _sender: &NSObject) {
            self.handle_browse();
        }

        #[unsafe(method(save:))]
        fn save(&self, _sender: &NSObject) {
            self.handle_save();
        }

        #[unsafe(method(cancel:))]
        fn cancel(&self, _sender: &NSObject) {
            self.finish(NSModalResponseCancel, None);
        }
    }
);

impl ConfigDialogDelegate {
    fn new(mtm: MainThreadMarker, base: PortableConfig) -> Retained<Self> {
        let ivars = DialogIvars {
            base,
            edit_base_url: OnceCell::new(),
            edit_api_key: OnceCell::new(),
            edit_model: OnceCell::new(),
            edit_provider: OnceCell::new(),
            edit_app_dir: OnceCell::new(),
            resolved: Cell::new(false),
            result: RefCell::new(None),
        };
        let this = Self::alloc(mtm).set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }

    fn handle_browse(&self) {
        let mtm = self.mtm();
        let Some(edit_app_dir) = self.ivars().edit_app_dir.get() else {
            return;
        };
        let current = edit_app_dir.stringValue().to_string();
        let start_dir = if current.trim().is_empty() {
            "/Applications".to_string()
        } else {
            current
        };

        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(true);
        panel.setAllowsMultipleSelection(false);
        panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(&start_dir))));

        if panel.runModal() == NSModalResponseOK
            && let Some(url) = panel.URL()
            && let Some(path) = url.path()
        {
            edit_app_dir.setStringValue(&path);
        }
    }

    fn handle_save(&self) {
        let ivars = self.ivars();
        let read = |field: Option<&Retained<NSTextField>>| {
            field.map(|f| f.stringValue().to_string()).unwrap_or_default()
        };
        let config = PortableConfig {
            api_base_url: read(ivars.edit_base_url.get()),
            api_key: ivars
                .edit_api_key
                .get()
                .map(|f| f.stringValue().to_string())
                .unwrap_or_default(),
            model: read(ivars.edit_model.get()),
            provider_name: read(ivars.edit_provider.get()),
            codex_app_dir: read(ivars.edit_app_dir.get()),
            debug_port: ivars.base.debug_port,
            last_synced_hash: ivars.base.last_synced_hash.clone(),
        };
        self.finish(NSModalResponseOK, Some(config));
    }

    fn finish(&self, code: NSModalResponse, result: Option<PortableConfig>) {
        if self.ivars().resolved.replace(true) {
            // Already resolved via a button click; this is the
            // `windowWillClose:` notification fired by our own explicit
            // `window.close()` afterwards. Ignore it so it doesn't clobber
            // the result that was just set.
            return;
        }
        *self.ivars().result.borrow_mut() = result;
        NSApplication::sharedApplication(self.mtm()).stopModalWithCode(code);
    }

    fn as_any_object(&self) -> &AnyObject {
        let obj: &NSObject = self;
        // SAFETY: `AnyObject` and `NSObject` are both plain Objective-C
        // object headers; every `NSObject` is a valid `AnyObject`.
        unsafe { &*(obj as *const NSObject as *const AnyObject) }
    }
}

/// Shows the configuration window and blocks until the user saves or closes it.
///
/// Returns `Some(config)` when the user clicked "保存并启动 Codex", or `None`
/// when the window was closed/cancelled (caller should not launch Codex).
pub fn show_portable_config_dialog(
    initial: &PortableConfig,
) -> anyhow::Result<Option<PortableConfig>> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| anyhow::anyhow!("portable config dialog must run on the main thread"))?;

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let delegate = ConfigDialogDelegate::new(mtm, initial.clone());
    let target = delegate.as_any_object();

    let content_view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(CONTENT_WIDTH, CONTENT_HEIGHT)),
    );

    let subtitle = NSTextField::labelWithString(ns_string!("填写 API 信息，保存后自动启动 Codex"), mtm);
    subtitle.setFrame(NSRect::new(
        NSPoint::new(PAD_X, CONTENT_HEIGHT - 34.0),
        NSSize::new(CONTENT_WIDTH - PAD_X * 2.0, 20.0),
    ));
    content_view.addSubview(&subtitle);

    // (label, row index, is password field, has a "浏览" button)
    let rows: [(&str, usize, bool, bool); 5] = [
        ("API 网址", 0, false, false),
        ("API Key", 1, true, false),
        ("默认模型", 2, false, false),
        ("Provider 名称", 3, false, false),
        ("Codex App 路径", 4, false, true),
    ];

    for (label_text, row, is_password, has_browse) in rows {
        let top = HEADER_HEIGHT + (row as f64) * ROW_PITCH;
        let y = CONTENT_HEIGHT - top - ROW_HEIGHT;

        let label = NSTextField::labelWithString(&NSString::from_str(label_text), mtm);
        label.setFrame(NSRect::new(
            NSPoint::new(PAD_X, y + 3.0),
            NSSize::new(LABEL_WIDTH, ROW_HEIGHT),
        ));
        content_view.addSubview(&label);

        let field_width = if has_browse { FIELD_WIDTH_WITH_BROWSE } else { FIELD_WIDTH };
        let initial_value = match row {
            0 => initial.api_base_url.as_str(),
            1 => initial.api_key.as_str(),
            2 => initial.model.as_str(),
            3 => initial.provider_name.as_str(),
            4 => initial.codex_app_dir.as_str(),
            _ => unreachable!(),
        };
        let frame = NSRect::new(NSPoint::new(FIELD_X, y), NSSize::new(field_width, ROW_HEIGHT));

        if is_password {
            let field = NSSecureTextField::initWithFrame(NSSecureTextField::alloc(mtm), frame);
            field.setBezeled(true);
            field.setEditable(true);
            field.setSelectable(true);
            field.setStringValue(&NSString::from_str(initial_value));
            content_view.addSubview(&field);
            let _ = delegate.ivars().edit_api_key.set(field);
        } else {
            let field = NSTextField::initWithFrame(NSTextField::alloc(mtm), frame);
            field.setBezeled(true);
            field.setEditable(true);
            field.setSelectable(true);
            field.setStringValue(&NSString::from_str(initial_value));
            content_view.addSubview(&field);
            let cell = match row {
                0 => &delegate.ivars().edit_base_url,
                2 => &delegate.ivars().edit_model,
                3 => &delegate.ivars().edit_provider,
                4 => &delegate.ivars().edit_app_dir,
                _ => unreachable!(),
            };
            let _ = cell.set(field);
        }

        if has_browse {
            let button = unsafe {
                NSButton::buttonWithTitle_target_action(
                    ns_string!("浏览"),
                    Some(target),
                    Some(sel!(browseAppDir:)),
                    mtm,
                )
            };
            button.setFrame(NSRect::new(
                NSPoint::new(FIELD_X + field_width + BROWSE_GAP, y),
                NSSize::new(BROWSE_WIDTH, ROW_HEIGHT),
            ));
            content_view.addSubview(&button);
        }
    }

    let save_width = 168.0;
    let cancel_width = 96.0;
    let button_gap = 10.0;
    let footer_y = (FOOTER_HEIGHT - BUTTON_HEIGHT) / 2.0;
    let field_right = FIELD_X + FIELD_WIDTH;
    let save_x = field_right - save_width;
    let cancel_x = save_x - button_gap - cancel_width;

    let cancel_button = unsafe {
        NSButton::buttonWithTitle_target_action(
            ns_string!("退出"),
            Some(target),
            Some(sel!(cancel:)),
            mtm,
        )
    };
    cancel_button.setFrame(NSRect::new(
        NSPoint::new(cancel_x, footer_y),
        NSSize::new(cancel_width, BUTTON_HEIGHT),
    ));
    content_view.addSubview(&cancel_button);

    let save_button = unsafe {
        NSButton::buttonWithTitle_target_action(
            ns_string!("保存并启动 Codex"),
            Some(target),
            Some(sel!(save:)),
            mtm,
        )
    };
    save_button.setFrame(NSRect::new(
        NSPoint::new(save_x, footer_y),
        NSSize::new(save_width, BUTTON_HEIGHT),
    ));
    content_view.addSubview(&save_button);

    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(CONTENT_WIDTH, CONTENT_HEIGHT)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: Disable auto-release when closing windows created outside a
    // window controller (mirrors the equivalent objc2 AppKit example).
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(ns_string!("Codex Launcher"));
    window.setContentView(Some(&content_view));
    window.center();
    window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    app.activate();
    window.makeKeyAndOrderFront(None);

    // Blocks until a button handler calls `stopModalWithCode`, or the window
    // is closed via the titlebar (which does the same via `windowWillClose:`).
    app.runModalForWindow(&window);
    window.close();

    // `runModalForWindow` pumps its own nested run loop while the dialog is
    // up, but that loop has already returned by the time we get here, so
    // nothing is left to flush the window's removal to the WindowServer.
    // Without a few more run loop turns, the (now logically closed) window
    // keeps showing on screen — frozen — for as long as this process stays
    // alive afterwards, which for the portable launcher can be indefinite
    // (it blocks on `wait_for_codex_exit` next). Give AppKit a brief moment
    // to actually flush the close before handing control back.
    let run_loop = NSRunLoop::currentRunLoop();
    for _ in 0..5 {
        run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.02));
    }

    // Nothing else in the portable launcher needs a Dock icon; drop it now
    // so the process doesn't keep showing as a foreground app in the Dock /
    // Cmd-Tab switcher while it sits in the background bridging Codex.
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);

    let result = delegate.ivars().result.borrow_mut().take();
    Ok(result)
}
