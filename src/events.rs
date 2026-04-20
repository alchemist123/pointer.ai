use cocoa::base::{id, nil, BOOL, YES};
use cocoa::foundation::NSString;
use objc::{class, msg_send, sel, sel_impl};
use std::ffi::c_void;
use std::sync::atomic::Ordering;
use std::thread;

use crate::state::*;
use crate::tour::{on_next_step_pressed, to_hidden, to_loading, toggle};

// ── Virtual-key codes ─────────────────────────────────────────────────────────

const VK_ANSI_P:   u16 = 35;
const VK_DELETE:   u16 = 51;
const VK_RETURN:   u16 = 36;
const VK_KP_ENTER: u16 = 76;
const VK_ESCAPE:   u16 = 53;

// ── CGEventTap constants ──────────────────────────────────────────────────────

const CGE_KEY_DOWN:                    u32 = 10;
const CGE_KEY_UP:                      u32 = 11;
const CGE_TAP_DISABLED_BY_TIMEOUT:    u32 = 0xFFFFFFFE;
const CGE_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFFFFFF;
const CGE_LEFT_MOUSE_DOWN:            u32 = 1;
const MASK_LEFT_MOUSE_DOWN: u64 = 1 << 1;
const MASK_KEY_DOWN:        u64 = 1 << 10;
const MASK_KEY_UP:          u64 = 1 << 11;
const NX_COMMAND_MASK:      u64 = 0x0010_0000;
const NX_SHIFT_MASK:        u64 = 0x0002_0000;
const KCG_KEYBOARD_EVENT_KEYCODE: i32 = 9;

// ── CoreGraphics / CoreFoundation ─────────────────────────────────────────────

extern "C" {
    fn CGEventTapCreate(
        tap: u32, place: u32, options: u32, eventsOfInterest: u64,
        callback: unsafe extern "C" fn(*const c_void, u32, *const c_void, *const c_void) -> *const c_void,
        userInfo: *const c_void,
    ) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetIntegerValueField(event: *const c_void, field: i32) -> i64;
    fn CGEventGetFlags(event: *const c_void) -> u64;
    fn CGEventGetLocation(event: *const c_void) -> cocoa::foundation::NSPoint;
    fn CGEventKeyboardGetUnicodeString(
        event: *const c_void, maxLen: usize, actualLen: *mut usize, buf: *mut u16,
    );
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void, port: *mut c_void, order: i32,
    ) -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopRun();
    static kCFRunLoopDefaultMode: *const c_void;
}

// ── Accessibility check ───────────────────────────────────────────────────────

pub fn ensure_accessibility() {
    unsafe {
        extern "C" {
            fn AXIsProcessTrusted() -> BOOL;
            fn AXIsProcessTrustedWithOptions(options: id) -> BOOL;
        }
        if AXIsProcessTrusted() == YES { return; }
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        eprintln!(
            "\n[pointer] Accessibility not granted.\n  Add: {exe}\n  \
             System Settings → Privacy & Security → Accessibility\n"
        );
        let key: id = NSString::alloc(nil).init_str("AXTrustedCheckOptionPrompt");
        let val: id = msg_send![class!(NSNumber), numberWithBool: YES];
        let opts: id = msg_send![class!(NSDictionary), dictionaryWithObject:val forKey:key];
        AXIsProcessTrustedWithOptions(opts);
    }
}

// ── Key handling (dispatched to main thread) ──────────────────────────────────

pub fn process_key(keycode: u16, chars: String) {
    match keycode {
        VK_DELETE => {
            let upd = INPUT_TEXT.get()
                .and_then(|m| m.lock().ok())
                .map(|mut g| { g.pop(); g.clone() });
            if let Some(s) = upd { update_display(&s); }
        }
        VK_RETURN | VK_KP_ENTER => to_loading(),
        VK_ESCAPE               => to_hidden(),
        _ => {
            let p: String = chars.chars().filter(|&c| c >= ' ' && c != '\x7f').collect();
            if !p.is_empty() {
                let upd = INPUT_TEXT.get()
                    .and_then(|m| m.lock().ok())
                    .map(|mut g| { g.push_str(&p); g.clone() });
                if let Some(s) = upd { update_display(&s); }
            }
        }
    }
}

// ── Event tap callback ────────────────────────────────────────────────────────

pub unsafe extern "C" fn event_tap_cb(
    _: *const c_void, ev_type: u32, event: *const c_void, _: *const c_void,
) -> *const c_void {
    if ev_type == CGE_TAP_DISABLED_BY_TIMEOUT || ev_type == CGE_TAP_DISABLED_BY_USER_INPUT {
        let tap = EVENT_TAP.load(Ordering::SeqCst) as *mut c_void;
        if !tap.is_null() { CGEventTapEnable(tap, true); }
        return event;
    }

    if ev_type == CGE_LEFT_MOUSE_DOWN {
        if APP_STATE.load(Ordering::SeqCst) == 4 && STEP_ACTION_CLICK.load(Ordering::SeqCst) {
            let loc = CGEventGetLocation(event);
            let tx = f64::from_bits(STEP_TARGET_CG_X.load(Ordering::SeqCst));
            let ty = f64::from_bits(STEP_TARGET_CG_Y.load(Ordering::SeqCst));
            let dx = loc.x - tx;
            let dy = loc.y - ty;
            if dx * dx + dy * dy < 80.0 * 80.0 {
                dispatch::Queue::main().exec_async(on_next_step_pressed);
            }
        }
        return event;
    }

    if ev_type != CGE_KEY_DOWN && ev_type != CGE_KEY_UP { return event; }

    let keycode = CGEventGetIntegerValueField(event, KCG_KEYBOARD_EVENT_KEYCODE) as u16;
    let flags   = CGEventGetFlags(event);
    let cmd     = flags & NX_COMMAND_MASK != 0;
    let shift   = flags & NX_SHIFT_MASK   != 0;
    let state   = APP_STATE.load(Ordering::SeqCst);

    if ev_type == CGE_KEY_DOWN {
        if keycode == VK_ANSI_P && cmd && shift {
            dispatch::Queue::main().exec_async(toggle);
            return std::ptr::null();
        }
        if state == 2 && !OVERLAY_ACTIVE.load(Ordering::SeqCst) && !cmd {
            let mut buf = [0u16; 4];
            let mut len: usize = 0;
            CGEventKeyboardGetUnicodeString(event, buf.len(), &mut len, buf.as_mut_ptr());
            let ch = String::from_utf16_lossy(&buf[..len]).to_owned();
            let kc = keycode;
            dispatch::Queue::main().exec_async(move || process_key(kc, ch));
            return std::ptr::null();
        }
        if keycode == VK_ESCAPE && state > 0 {
            dispatch::Queue::main().exec_async(to_hidden);
        }
    }
    if state == 2 && !OVERLAY_ACTIVE.load(Ordering::SeqCst) && !cmd && ev_type == CGE_KEY_UP {
        return std::ptr::null();
    }
    event
}

// ── Event-tap run loop (blocking — call from a background thread) ─────────────

pub fn run_event_tap() {
    unsafe {
        loop {
            let tap = CGEventTapCreate(
                0, 0, 0,
                MASK_LEFT_MOUSE_DOWN | MASK_KEY_DOWN | MASK_KEY_UP,
                event_tap_cb,
                std::ptr::null(),
            );
            if tap.is_null() {
                eprintln!(
                    "[pointer] Event tap failed — grant Accessibility permission. Retrying in 2s…"
                );
                thread::sleep(std::time::Duration::from_secs(2));
                continue;
            }
            EVENT_TAP.store(tap as usize, Ordering::SeqCst);
            let src = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopDefaultMode);
            CFRunLoopRun();
            EVENT_TAP.store(0, Ordering::SeqCst);
        }
    }
}
