#![allow(non_snake_case)]

use cocoa::base::{id, nil, NO, YES};
use cocoa::foundation::{NSPoint, NSRect, NSString};
use objc::{class, msg_send, sel, sel_impl};
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering,
};
use std::sync::{Mutex, OnceLock};

use crate::agent::TourAgent;
use crate::config::{MsPtrs, PointerConfig};

// ── Constants ─────────────────────────────────────────────────────────────────

pub const DOT_SZ: f64 = 40.0;
pub const IN_W:   f64 = 360.0;
pub const IN_H:   f64 = 52.0;
pub const STEP_W: f64 = 320.0;
pub const STEP_H: f64 = 210.0;
pub const HINT_W: f64 = 168.0;
pub const HINT_H: f64 = 26.0;

// ── App state machine values ──────────────────────────────────────────────────
// 0 = hidden, 1 = dot, 2 = input, 3 = loading, 4 = step, 5 = done

pub static APP_STATE:      AtomicU8   = AtomicU8::new(0);
pub static BLINK_TICK:     AtomicU32  = AtomicU32::new(0);
pub static BLINK_ALPHA:    AtomicU32  = AtomicU32::new(0);
pub static LOAD_TICK:      AtomicU32  = AtomicU32::new(0);
pub static CURSOR_TICK:    AtomicU32  = AtomicU32::new(0);
pub static CURSOR_ON:      AtomicBool = AtomicBool::new(true);
pub static DONE_TICK:      AtomicU32  = AtomicU32::new(0);
pub static EVENT_TAP:      AtomicUsize = AtomicUsize::new(0);
pub static OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(false);

// ── Window / view pointers ────────────────────────────────────────────────────

pub static DOT_WIN_PTR:        OnceLock<usize>         = OnceLock::new();
pub static DOT_VIEW_PTR:       OnceLock<usize>         = OnceLock::new();
pub static IN_PANEL_PTR:       OnceLock<usize>         = OnceLock::new();
pub static INPUT_VIEW_PTR:     OnceLock<usize>         = OnceLock::new();
pub static DISPLAY_FIELD_PTR:  OnceLock<usize>         = OnceLock::new();
pub static DISPLAY_SCROLL_PTR: OnceLock<usize>         = OnceLock::new();
pub static PLACEHOLDER_PTR:    OnceLock<usize>         = OnceLock::new();
pub static INPUT_TEXT:         OnceLock<Mutex<String>> = OnceLock::new();
pub static LAST_CONTEXT:       OnceLock<Mutex<(String, String)>> = OnceLock::new();

// Step panel
pub static STEP_WIN_PTR:      OnceLock<usize> = OnceLock::new();
pub static STEP_HEAD_PTR:     OnceLock<usize> = OnceLock::new();
pub static STEP_BODY_PTR:     OnceLock<usize> = OnceLock::new();
pub static STEP_REASON_PTR:   OnceLock<usize> = OnceLock::new();
pub static STEP_NEXT_BTN_PTR: OnceLock<usize> = OnceLock::new();
pub static STEP_IS_FINAL:     AtomicBool      = AtomicBool::new(false);
pub static STEP_ACTION_CLICK: AtomicBool      = AtomicBool::new(false);
pub static TOUR_AGENT:        OnceLock<Mutex<Option<TourAgent>>> = OnceLock::new();

// Active cursor screen (set in to_dot; used for capture + coordinate math)
pub static CURSOR_SCREEN_ORIGIN_X: AtomicU64 = AtomicU64::new(0);
pub static CURSOR_SCREEN_ORIGIN_Y: AtomicU64 = AtomicU64::new(0);
pub static CURSOR_SCREEN_W:        AtomicU64 = AtomicU64::new(0);
pub static CURSOR_SCREEN_H:        AtomicU64 = AtomicU64::new(0);
pub static CURSOR_DISPLAY_IDX:     AtomicU32 = AtomicU32::new(1);

// Primary screen height (needed for AppKit → CG coord conversion in show_step)
pub static SCREEN_H_LOGICAL: AtomicU64 = AtomicU64::new(0);
pub static STEP_TARGET_CG_X: AtomicU64 = AtomicU64::new(0);
pub static STEP_TARGET_CG_Y: AtomicU64 = AtomicU64::new(0);
pub static ANIM_FROM_X:      AtomicU64 = AtomicU64::new(0);
pub static ANIM_FROM_Y:      AtomicU64 = AtomicU64::new(0);
pub static ANIM_TO_X:        AtomicU64 = AtomicU64::new(0);
pub static ANIM_TO_Y:        AtomicU64 = AtomicU64::new(0);
pub static ANIM_TICK:        AtomicU32 = AtomicU32::new(0);

// Completion panel
pub static COMPLETE_COUNTDOWN: AtomicI32           = AtomicI32::new(-1);
pub static GOAL_TEXT:          OnceLock<Mutex<String>> = OnceLock::new();
pub static FINAL_ANSWER:       OnceLock<Mutex<String>> = OnceLock::new();

// Colour picker + settings
pub static DOT_COLOR:      AtomicU8            = AtomicU8::new(0);
pub static HINT_WIN_PTR:   OnceLock<usize>     = OnceLock::new();
pub static HINT_LBL_PTR:   OnceLock<usize>     = OnceLock::new();
pub static COLOR_WIN_PTR:  OnceLock<usize>     = OnceLock::new();
pub static COLOR_BTN_PTRS: OnceLock<[usize; 8]> = OnceLock::new();
pub static MODEL_WIN_PTR:  OnceLock<usize>     = OnceLock::new();
pub static MS_PTRS:        OnceLock<MsPtrs>    = OnceLock::new();
pub static POINTER_CONFIG: OnceLock<Mutex<PointerConfig>> = OnceLock::new();

// ── Window accessors ──────────────────────────────────────────────────────────

#[inline] pub unsafe fn dot_win()       -> id { *DOT_WIN_PTR.get().unwrap()       as id }
#[inline] pub unsafe fn dot_view()      -> id { *DOT_VIEW_PTR.get().unwrap()      as id }
#[inline] pub unsafe fn in_panel()      -> id { *IN_PANEL_PTR.get().unwrap()      as id }
#[inline] pub unsafe fn input_view()    -> id { *INPUT_VIEW_PTR.get().unwrap()    as id }
#[inline] pub unsafe fn display_field() -> id { *DISPLAY_FIELD_PTR.get().unwrap() as id }
#[inline] pub unsafe fn placeholder()   -> id { *PLACEHOLDER_PTR.get().unwrap()   as id }
#[inline] pub unsafe fn step_win()      -> id { *STEP_WIN_PTR.get().unwrap()      as id }
#[inline] pub unsafe fn hint_win()      -> id { *HINT_WIN_PTR.get().unwrap()      as id }
#[inline] pub unsafe fn color_win()     -> id { *COLOR_WIN_PTR.get().unwrap()     as id }
#[inline] pub unsafe fn model_win()     -> id { *MODEL_WIN_PTR.get().unwrap()     as id }

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn dot_color_rgb() -> (f64, f64, f64) {
    match DOT_COLOR.load(Ordering::SeqCst) {
        1 => (1.00, 0.22, 0.15), // red
        2 => (0.12, 0.75, 0.30), // green
        3 => (1.00, 0.55, 0.05), // orange
        4 => (0.65, 0.20, 0.90), // purple
        5 => (1.00, 0.25, 0.60), // pink
        6 => (0.00, 0.75, 0.75), // teal
        7 => (0.95, 0.80, 0.00), // yellow
        _ => (0.10, 0.50, 1.00), // blue (default)
    }
}

pub unsafe fn force_dark(view: id) {
    let name: id = NSString::alloc(nil).init_str("NSAppearanceNameDarkAqua");
    let dark: id = msg_send![class!(NSAppearance), appearanceNamed: name];
    let _: () = msg_send![view, setAppearance: dark];
}

#[inline]
pub unsafe fn redraw_dot() {
    let _: () = msg_send![dot_view(), setNeedsDisplay: YES];
}

pub fn update_display(text: &str) {
    unsafe {
        let s = if APP_STATE.load(Ordering::SeqCst) == 2 && CURSOR_ON.load(Ordering::SeqCst) {
            format!("{text}\u{2502}")
        } else {
            text.to_owned()
        };
        let ns: id = NSString::alloc(nil).init_str(&s);
        let _: () = msg_send![display_field(), setStringValue: ns];
        let _: () = msg_send![display_field(), sizeToFit];
        if let Some(&sp) = DISPLAY_SCROLL_PTR.get() {
            let scroll: id = sp as id;
            let df_frame: NSRect = msg_send![display_field(), frame];
            let vis: NSRect     = msg_send![scroll, documentVisibleRect];
            let x = (df_frame.size.width - vis.size.width).max(0.0);
            let clip: id = msg_send![scroll, contentView];
            let _: () = msg_send![clip, scrollToPoint: NSPoint::new(x, 0.0)];
            let _: () = msg_send![scroll, reflectScrolledClipView: clip];
        }
        let _: () = msg_send![placeholder(), setHidden: if text.is_empty() { NO } else { YES }];
    }
}

pub unsafe fn ns_str(s: id) -> String {
    if s.is_null() { return String::new(); }
    let ptr: *const std::os::raw::c_char = msg_send![s, UTF8String];
    if ptr.is_null() { return String::new(); }
    std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
}
