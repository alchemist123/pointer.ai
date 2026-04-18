#![allow(non_snake_case)]

mod agent;
use agent::{AgentStep, TourAgent};

use cocoa::appkit::{
    NSApp, NSApplication, NSApplicationActivationPolicyAccessory,
    NSBackingStoreType, NSWindow, NSWindowStyleMask,
};
use cocoa::base::{id, nil, BOOL, NO, YES};
use cocoa::foundation::{NSAutoreleasePool, NSPoint, NSRect, NSSize, NSString};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;

// ── Constants ─────────────────────────────────────────────────────────────────

const DOT_SZ: f64 = 44.0;
const IN_W:   f64 = 260.0;
const IN_H:   f64 = 72.0;
const STEP_W: f64 = 310.0;
const STEP_H: f64 = 172.0;

// ── State ─────────────────────────────────────────────────────────────────────
// 0 = hidden
// 1 = dot (blinking, click to open input)
// 2 = input (typing query)
// 3 = loading (AI thinking, spinner)
// 4 = step  (dot at target + step panel)
// 5 = done  (completion panel, auto-dismiss)

static APP_STATE:     AtomicU8  = AtomicU8::new(0);
static BLINK_TICK:    AtomicU32 = AtomicU32::new(0);
static BLINK_ALPHA:   AtomicU32 = AtomicU32::new(0);
static LOAD_TICK:     AtomicU32 = AtomicU32::new(0);
static CURSOR_TICK:   AtomicU32 = AtomicU32::new(0);
static CURSOR_ON:     AtomicBool = AtomicBool::new(true);
static DONE_TICK:     AtomicU32 = AtomicU32::new(0);
static EVENT_TAP:     AtomicUsize = AtomicUsize::new(0);

static DOT_WIN_PTR:       OnceLock<usize> = OnceLock::new();
static DOT_VIEW_PTR:      OnceLock<usize> = OnceLock::new();
static IN_PANEL_PTR:      OnceLock<usize> = OnceLock::new();
static INPUT_VIEW_PTR:    OnceLock<usize> = OnceLock::new();
static DISPLAY_FIELD_PTR: OnceLock<usize> = OnceLock::new();
static PLACEHOLDER_PTR:   OnceLock<usize> = OnceLock::new();
static INPUT_TEXT:        OnceLock<Mutex<String>> = OnceLock::new();
static LAST_CONTEXT:      OnceLock<Mutex<(String, String)>> = OnceLock::new();

// Step panel
static STEP_WIN_PTR:      OnceLock<usize> = OnceLock::new();
static STEP_HEAD_PTR:     OnceLock<usize> = OnceLock::new();
static STEP_BODY_PTR:     OnceLock<usize> = OnceLock::new();
static STEP_NEXT_BTN_PTR: OnceLock<usize> = OnceLock::new();
static STEP_IS_FINAL:       AtomicBool = AtomicBool::new(false);
static STEP_ACTION_CLICK:   AtomicBool = AtomicBool::new(false);
static TOUR_AGENT:          OnceLock<Mutex<Option<TourAgent>>> = OnceLock::new();

// Smooth dot animation (NS logical coords)
static SCREEN_H_LOGICAL: AtomicU64 = AtomicU64::new(0);
static STEP_TARGET_CG_X: AtomicU64 = AtomicU64::new(0);
static STEP_TARGET_CG_Y: AtomicU64 = AtomicU64::new(0);
static ANIM_FROM_X:      AtomicU64 = AtomicU64::new(0);
static ANIM_FROM_Y:      AtomicU64 = AtomicU64::new(0);
static ANIM_TO_X:        AtomicU64 = AtomicU64::new(0);
static ANIM_TO_Y:        AtomicU64 = AtomicU64::new(0);
static ANIM_TICK:        AtomicU32 = AtomicU32::new(0);

// Completion flash: counts down ticks before showing the completion panel (-1 = idle)
static COMPLETE_COUNTDOWN: AtomicI32 = AtomicI32::new(-1);
static GOAL_TEXT: OnceLock<Mutex<String>> = OnceLock::new();

// ── Accessors ─────────────────────────────────────────────────────────────────

#[inline] unsafe fn dot_win()       -> id { *DOT_WIN_PTR.get().unwrap()       as id }
#[inline] unsafe fn dot_view()      -> id { *DOT_VIEW_PTR.get().unwrap()      as id }
#[inline] unsafe fn in_panel()      -> id { *IN_PANEL_PTR.get().unwrap()      as id }
#[inline] unsafe fn input_view()    -> id { *INPUT_VIEW_PTR.get().unwrap()    as id }
#[inline] unsafe fn display_field() -> id { *DISPLAY_FIELD_PTR.get().unwrap() as id }
#[inline] unsafe fn placeholder()   -> id { *PLACEHOLDER_PTR.get().unwrap()   as id }
#[inline] unsafe fn step_win()      -> id { *STEP_WIN_PTR.get().unwrap()      as id }

#[inline] unsafe fn redraw_dot() {
    let _: () = msg_send![dot_view(), setNeedsDisplay: YES];
}

fn update_display(text: &str) {
    unsafe {
        let s = if APP_STATE.load(Ordering::SeqCst) == 2 && CURSOR_ON.load(Ordering::SeqCst) {
            format!("{text}\u{2502}")
        } else {
            text.to_owned()
        };
        let ns: id = NSString::alloc(nil).init_str(&s);
        let _: () = msg_send![display_field(), setStringValue: ns];
        let _: () = msg_send![placeholder(), setHidden: if text.is_empty() { NO } else { YES }];
    }
}

// ── State machine ─────────────────────────────────────────────────────────────

fn toggle()     { if APP_STATE.load(Ordering::SeqCst) == 0 { unsafe { to_dot() } } else { to_hidden() } }
fn to_hidden()  { unsafe { to_hidden_impl()  } }
fn to_input()   { unsafe { to_input_impl()   } }
fn to_loading() { unsafe { to_loading_impl() } }

// ── Context capture ───────────────────────────────────────────────────────────

unsafe fn ns_str(s: id) -> String {
    if s.is_null() { return String::new(); }
    let ptr: *const std::os::raw::c_char = msg_send![s, UTF8String];
    if ptr.is_null() { return String::new(); }
    std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
}

unsafe fn focused_window_title(pid: i32) -> String {
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> *const c_void;
        fn AXUIElementCopyAttributeValue(
            element:   *const c_void,
            attribute: *const c_void,
            value:     *mut *const c_void,
        ) -> i32;
        fn CFStringGetCString(s: *const c_void, buf: *mut i8, size: i32, enc: u32) -> BOOL;
    }
    const CF_UTF8: u32 = 0x0800_0100;
    let ax = AXUIElementCreateApplication(pid);
    if ax.is_null() { return String::new(); }
    let attr_w: id = NSString::alloc(nil).init_str("AXFocusedWindow");
    let mut win: *const c_void = std::ptr::null();
    if AXUIElementCopyAttributeValue(ax, attr_w as _, &mut win) != 0 || win.is_null() {
        return String::new();
    }
    let attr_t: id = NSString::alloc(nil).init_str("AXTitle");
    let mut tref: *const c_void = std::ptr::null();
    if AXUIElementCopyAttributeValue(win, attr_t as _, &mut tref) != 0 || tref.is_null() {
        return String::new();
    }
    let mut buf = [0i8; 512];
    if CFStringGetCString(tref, buf.as_mut_ptr(), 512, CF_UTF8) == NO { return String::new(); }
    std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
}

unsafe fn capture_context() {
    let ws: id    = msg_send![class!(NSWorkspace), sharedWorkspace];
    let front: id = msg_send![ws, frontmostApplication];
    let app  = if !front.is_null() { ns_str(msg_send![front, localizedName]) } else { String::new() };
    let pid: i32 = if !front.is_null() { msg_send![front, processIdentifier] } else { 0 };
    let win  = if pid > 0 { focused_window_title(pid) } else { String::new() };
    if let Some(m) = LAST_CONTEXT.get() {
        if let Ok(mut g) = m.lock() { *g = (app, win); }
    }
}

// ── State transitions ─────────────────────────────────────────────────────────

unsafe fn to_dot() {
    capture_context();
    let mouse: NSPoint = msg_send![class!(NSEvent), mouseLocation];
    let _: () = msg_send![dot_win(),
        setFrame: NSRect::new(
            NSPoint::new(mouse.x - DOT_SZ/2.0, mouse.y - DOT_SZ/2.0),
            NSSize::new(DOT_SZ, DOT_SZ))
        display: YES
    ];
    let _: () = msg_send![dot_win(), setIgnoresMouseEvents: NO];
    BLINK_TICK.store(0, Ordering::SeqCst);
    redraw_dot();
    let _: () = msg_send![dot_win(), orderFrontRegardless];
    APP_STATE.store(1, Ordering::SeqCst);
}

unsafe fn to_hidden_impl() {
    let _: () = msg_send![dot_win(),  orderOut: nil as id];
    let _: () = msg_send![in_panel(), orderOut: nil as id];
    let _: () = msg_send![step_win(), orderOut: nil as id];
    let _: () = msg_send![dot_win(),  setIgnoresMouseEvents: NO];
    if let Some(m) = INPUT_TEXT.get()  { if let Ok(mut g) = m.lock() { g.clear(); } }
    if let Some(m) = TOUR_AGENT.get()  { if let Ok(mut g) = m.lock() { *g = None; } }
    APP_STATE.store(0, Ordering::SeqCst);
}

unsafe fn to_input_impl() {
    let df: NSRect = msg_send![dot_win(), frame];
    let screen: id = msg_send![class!(NSScreen), mainScreen];
    let sf: NSRect = msg_send![screen, frame];
    let mut ix = df.origin.x + (DOT_SZ - IN_W) / 2.0;
    let mut iy = df.origin.y + DOT_SZ + 6.0;
    ix = ix.max(8.0).min(sf.size.width  - IN_W - 8.0);
    iy = iy.max(8.0).min(sf.size.height - IN_H - 8.0);

    let _: () = msg_send![dot_win(), orderOut: nil as id];
    let _: () = msg_send![in_panel(),
        setFrame: NSRect::new(NSPoint::new(ix, iy), NSSize::new(IN_W, IN_H))
        display: YES
    ];
    CURSOR_TICK.store(0, Ordering::SeqCst);
    CURSOR_ON.store(true, Ordering::SeqCst);
    APP_STATE.store(2, Ordering::SeqCst);
    update_display("");
    let _: () = msg_send![in_panel(), makeKeyAndOrderFront: nil as id];
    let _: () = msg_send![in_panel(), makeFirstResponder: input_view()];
}

unsafe fn to_loading_impl() {
    let query = INPUT_TEXT.get().and_then(|m| m.lock().ok()).map(|g| g.clone()).unwrap_or_default();
    let (app, win) = LAST_CONTEXT.get().and_then(|m| m.lock().ok()).map(|g| g.clone()).unwrap_or_default();

    // Get browser URL while context is fresh.
    let url = agent::get_browser_url(&app);

    println!("app:    {app}");
    println!("window: {win}");
    if let Some(ref u) = url { println!("url:    {u}"); }
    println!("query:  {query}");

    if let Some(m) = GOAL_TEXT.get() { if let Ok(mut g) = m.lock() { *g = query.clone(); } }
    if let Some(m) = INPUT_TEXT.get() { if let Ok(mut g) = m.lock() { g.clear(); } }
    update_display("");
    let _: () = msg_send![in_panel(), orderOut: nil as id];
    let _: () = msg_send![step_win(), orderOut: nil as id];

    LOAD_TICK.store(0, Ordering::SeqCst);
    APP_STATE.store(3, Ordering::SeqCst);
    let _: () = msg_send![dot_win(), orderFrontRegardless];
    redraw_dot();

    // Use primary display dimensions — matches screencapture -m.
    let screens: id = msg_send![class!(NSScreen), screens];
    let primary: id = msg_send![screens, objectAtIndex: 0usize];
    let sf: NSRect  = msg_send![primary, frame];

    SCREEN_H_LOGICAL.store(sf.size.height.to_bits(), Ordering::SeqCst);
    let new_agent = TourAgent::new(query, app, win, url, sf.size.width, sf.size.height);
    if let Some(m) = TOUR_AGENT.get() { if let Ok(mut g) = m.lock() { *g = Some(new_agent); } }
    fire_next_step();
}

// ── Tour-guide helpers ────────────────────────────────────────────────────────

fn fire_next_step() {
    let agent = match TOUR_AGENT.get().and_then(|m| m.lock().ok()) {
        Some(mut g) => match g.take() {
            Some(a) => a,
            None => { eprintln!("[pointer] fire_next_step: agent is None — aborting"); to_hidden(); return; }
        },
        None => { eprintln!("[pointer] fire_next_step: TOUR_AGENT not initialised"); return; }
    };
    agent::spawn(
        move || { let mut a = agent; let r = a.next_step(); (a, r) },
        |(agent, result)| {
            if let Some(m) = TOUR_AGENT.get() {
                if let Ok(mut g) = m.lock() { *g = Some(agent); }
            }
            match result {
                Ok(step) => show_step(step),
                Err(e)   => { eprintln!("[pointer] Agent error: {e}"); to_hidden(); }
            }
        },
    );
}

fn show_step(step: AgentStep) {
    unsafe {
        // Animate dot from current position to AI target.
        let curr: NSRect = msg_send![dot_win(), frame];
        let from_x = curr.origin.x + DOT_SZ / 2.0;
        let from_y = curr.origin.y + DOT_SZ / 2.0;
        ANIM_FROM_X.store(from_x.to_bits(), Ordering::SeqCst);
        ANIM_FROM_Y.store(from_y.to_bits(), Ordering::SeqCst);
        ANIM_TO_X.store(step.x.to_bits(), Ordering::SeqCst);
        ANIM_TO_Y.store(step.y.to_bits(), Ordering::SeqCst);
        ANIM_TICK.store(0, Ordering::SeqCst);
        // Store CG-coords target for click auto-advance detection.
        let sh = f64::from_bits(SCREEN_H_LOGICAL.load(Ordering::SeqCst));
        STEP_TARGET_CG_X.store(step.x.to_bits(), Ordering::SeqCst);
        STEP_TARGET_CG_Y.store((sh - step.y).to_bits(), Ordering::SeqCst);
        // Dot is click-through in state 4 — user clicks the REAL element, not the dot.
        let _: () = msg_send![dot_win(), setIgnoresMouseEvents: YES];
        // Track whether this is a Click action for auto-advance.
        let is_click = matches!(step.action.as_str(), "Click" | "Double-click" | "Right-click");
        STEP_ACTION_CLICK.store(is_click, Ordering::SeqCst);

        // Update step panel.
        let head: id = *STEP_HEAD_PTR.get().unwrap() as id;
        let body: id = *STEP_BODY_PTR.get().unwrap() as id;
        let btn:  id = *STEP_NEXT_BTN_PTR.get().unwrap() as id;

        let head_txt = format!("Step {}  ·  {}", step.step_num, step.action.to_uppercase());
        let _: () = msg_send![head, setStringValue: NSString::alloc(nil).init_str(&head_txt)];
        let _: () = msg_send![body, setStringValue: NSString::alloc(nil).init_str(&step.description)];

        // Button text + colour based on whether this is the final step.
        let (btn_title, r, g, b) = if step.is_final {
            ("Done  ✓", 0.10_f64, 0.55_f64, 0.30_f64)
        } else {
            ("I did it  →", 0.07_f64, 0.47_f64, 1.00_f64)
        };
        let _: () = msg_send![btn, setTitle: NSString::alloc(nil).init_str(btn_title)];
        let btn_layer: id = msg_send![btn, layer];
        let col: id = msg_send![class!(NSColor), colorWithRed:r green:g blue:b alpha:1.0_f64];
        let cg:  id = msg_send![col, CGColor];
        let _: () = msg_send![btn_layer, setBackgroundColor: cg];

        // Position step panel: above dot if room, else below.
        let screen: id = msg_send![class!(NSScreen), mainScreen];
        let sf: NSRect  = msg_send![screen, frame];
        let px = (step.x - STEP_W/2.0).max(8.0).min(sf.size.width  - STEP_W - 8.0);
        let py_above = step.y + DOT_SZ/2.0 + 14.0;
        let py_below = step.y - DOT_SZ/2.0 - STEP_H - 14.0;
        let py = if py_above + STEP_H < sf.size.height - 8.0 { py_above } else { py_below.max(8.0) };
        let _: () = msg_send![step_win(),
            setFrame: NSRect::new(NSPoint::new(px, py), NSSize::new(STEP_W, STEP_H))
            display: YES
        ];

        STEP_IS_FINAL.store(step.is_final, Ordering::SeqCst);
        BLINK_TICK.store(0, Ordering::SeqCst);
        APP_STATE.store(4, Ordering::SeqCst);
        let _: () = msg_send![step_win(), orderFrontRegardless];
        let _: () = msg_send![dot_win(),  orderFrontRegardless];
        redraw_dot();
    }
}

/// Called when user clicks "I did it →" / "Done ✓" or auto-advance triggers.
fn on_next_step_pressed() {
    let state = APP_STATE.load(Ordering::SeqCst);
    // If already on the completion panel the button means "close".
    if state == 5 { to_hidden(); return; }

    if STEP_IS_FINAL.load(Ordering::SeqCst) {
        // Guard: don't retrigger if countdown already running.
        if COMPLETE_COUNTDOWN.load(Ordering::SeqCst) >= 0 { return; }
        // Flash "step done!" in the panel, then let the pulse trigger show_completion().
        unsafe {
            let head: id = *STEP_HEAD_PTR.get().unwrap() as id;
            let body: id = *STEP_BODY_PTR.get().unwrap() as id;
            let btn:  id = *STEP_NEXT_BTN_PTR.get().unwrap() as id;
            let _: () = msg_send![head, setStringValue:
                NSString::alloc(nil).init_str("✓  Step done!")];
            let _: () = msg_send![body, setStringValue:
                NSString::alloc(nil).init_str("Nice work! Wrapping up…")];
            let _: () = msg_send![btn, setTitle: NSString::alloc(nil).init_str("✓")];
            let btn_layer: id = msg_send![btn, layer];
            let green: id = msg_send![class!(NSColor),
                colorWithRed:0.10_f64 green:0.58_f64 blue:0.32_f64 alpha:1.0_f64];
            let cg: id = msg_send![green, CGColor];
            let _: () = msg_send![btn_layer, setBackgroundColor: cg];
        }
        COMPLETE_COUNTDOWN.store(14, Ordering::SeqCst); // ~700 ms
        return;
    }
    unsafe {
        let _: () = msg_send![step_win(), orderOut: nil as id];
        let _: () = msg_send![dot_win(),  setIgnoresMouseEvents: NO];
        LOAD_TICK.store(0, Ordering::SeqCst);
        APP_STATE.store(3, Ordering::SeqCst);
        let _: () = msg_send![dot_win(), orderFrontRegardless];
        redraw_dot();
    }
    fire_next_step();
}

unsafe fn show_completion() {
    let _: () = msg_send![dot_win(), orderOut: nil as id];

    let goal = GOAL_TEXT.get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_default();

    let head: id = *STEP_HEAD_PTR.get().unwrap() as id;
    let body: id = *STEP_BODY_PTR.get().unwrap() as id;
    let btn:  id = *STEP_NEXT_BTN_PTR.get().unwrap() as id;

    let _: () = msg_send![head, setStringValue: NSString::alloc(nil).init_str("🎉  All done!")];
    let body_text = if goal.is_empty() {
        "You completed all the steps — great job!".to_owned()
    } else {
        format!("You completed: {}", goal)
    };
    let _: () = msg_send![body, setStringValue:
        NSString::alloc(nil).init_str(&body_text)];
    let _: () = msg_send![btn, setTitle: NSString::alloc(nil).init_str("Close guide")];
    let btn_layer: id = msg_send![btn, layer];
    let green: id = msg_send![class!(NSColor),
        colorWithRed:0.10_f64 green:0.58_f64 blue:0.32_f64 alpha:1.0_f64];
    let cg: id = msg_send![green, CGColor];
    let _: () = msg_send![btn_layer, setBackgroundColor: cg];

    let screen: id = msg_send![class!(NSScreen), mainScreen];
    let sf: NSRect  = msg_send![screen, frame];
    let px = (sf.size.width  - STEP_W) / 2.0;
    let py = (sf.size.height - STEP_H) / 2.0 + 60.0;
    let _: () = msg_send![step_win(),
        setFrame: NSRect::new(NSPoint::new(px, py), NSSize::new(STEP_W, STEP_H))
        display: YES
    ];
    let _: () = msg_send![step_win(), orderFrontRegardless];

    DONE_TICK.store(0, Ordering::SeqCst);
    APP_STATE.store(5, Ordering::SeqCst);
}

// ── PointerView ───────────────────────────────────────────────────────────────

unsafe fn register_pointer_view() -> *const Class {
    let mut decl = ClassDecl::new("PointerView", class!(NSView))
        .expect("PointerView already declared");

    extern "C" fn is_opaque(_: &Object, _: Sel) -> BOOL { NO }

    extern "C" fn draw_rect(this: &Object, _: Sel, _: NSRect) {
        unsafe {
            let me = this as *const Object as id;
            let b: NSRect = msg_send![me, bounds];
            let cx = b.size.width / 2.0;
            let cy = b.size.height / 2.0;
            match APP_STATE.load(Ordering::SeqCst) {
                1 | 4 => {
                    let a = f32::from_bits(BLINK_ALPHA.load(Ordering::SeqCst)) as f64;
                    let t = (a - 0.22) / 0.71;
                    // Glow
                    let gr = 17.0 + 7.0 * t;
                    let glow: id = msg_send![class!(NSBezierPath),
                        bezierPathWithOvalInRect: NSRect::new(
                            NSPoint::new(cx-gr, cy-gr), NSSize::new(gr*2.0, gr*2.0))];
                    let gc: id = msg_send![class!(NSColor),
                        colorWithRed:0.20 green:0.55 blue:1.00 alpha: a*0.28];
                    let _: () = msg_send![gc, set]; let _: () = msg_send![glow, fill];
                    // Ring
                    let rr = 10.0 + 2.0 * t;
                    let ring: id = msg_send![class!(NSBezierPath),
                        bezierPathWithOvalInRect: NSRect::new(
                            NSPoint::new(cx-rr, cy-rr), NSSize::new(rr*2.0, rr*2.0))];
                    let _: () = msg_send![ring, setLineWidth: 1.5_f64];
                    let rc: id = msg_send![class!(NSColor),
                        colorWithRed:0.25 green:0.60 blue:1.00 alpha: a*0.75];
                    let _: () = msg_send![rc, set]; let _: () = msg_send![ring, stroke];
                    // Core
                    let ir = 5.0 + 1.5 * t;
                    let dot: id = msg_send![class!(NSBezierPath),
                        bezierPathWithOvalInRect: NSRect::new(
                            NSPoint::new(cx-ir, cy-ir), NSSize::new(ir*2.0, ir*2.0))];
                    let dc: id = msg_send![class!(NSColor),
                        colorWithRed:0.10 green:0.50 blue:1.00 alpha: a];
                    let _: () = msg_send![dc, set]; let _: () = msg_send![dot, fill];
                    // Specular
                    let sa = ((a - 0.60) * 2.5).max(0.0).min(1.0) * 0.60;
                    if sa > 0.01 {
                        let wr = 2.5_f64;
                        let sp: id = msg_send![class!(NSBezierPath),
                            bezierPathWithOvalInRect: NSRect::new(
                                NSPoint::new(cx-wr+2.0, cy-wr+2.0), NSSize::new(wr*2.0, wr*2.0))];
                        let wc: id = msg_send![class!(NSColor),
                            colorWithRed:1.0 green:1.0 blue:1.0 alpha:sa];
                        let _: () = msg_send![wc, set]; let _: () = msg_send![sp, fill];
                    }
                }
                3 => {
                    let tick = LOAD_TICK.load(Ordering::SeqCst);
                    let ir = 5.0_f64;
                    let dot: id = msg_send![class!(NSBezierPath),
                        bezierPathWithOvalInRect: NSRect::new(
                            NSPoint::new(cx-ir, cy-ir), NSSize::new(ir*2.0, ir*2.0))];
                    let dc: id = msg_send![class!(NSColor),
                        colorWithRed:0.10 green:0.50 blue:1.00 alpha:0.35];
                    let _: () = msg_send![dc, set]; let _: () = msg_send![dot, fill];
                    let start = 90.0 - (tick as f64 * 9.0) % 360.0;
                    let arc: id = msg_send![class!(NSBezierPath), bezierPath];
                    let _: () = msg_send![arc,
                        appendBezierPathWithArcWithCenter: NSPoint::new(cx, cy)
                        radius: 13.0_f64 startAngle: start endAngle: start-270.0 clockwise: YES];
                    let _: () = msg_send![arc, setLineWidth: 2.5_f64];
                    let ac: id = msg_send![class!(NSColor),
                        colorWithRed:0.15 green:0.55 blue:1.00 alpha:0.90];
                    let _: () = msg_send![ac, set]; let _: () = msg_send![arc, stroke];
                }
                _ => {}
            }
        }
    }

    // Only hittable in state 1 (open input on click).
    extern "C" fn hit_test(this: &Object, _: Sel, pt: NSPoint) -> id {
        unsafe {
            if APP_STATE.load(Ordering::SeqCst) != 1 { return std::ptr::null_mut(); }
            let me = this as *const Object as id;
            let lp: NSPoint = msg_send![me, convertPoint: pt fromView: nil as id];
            let b:  NSRect  = msg_send![me, bounds];
            let dx = lp.x - b.size.width / 2.0;
            let dy = lp.y - b.size.height / 2.0;
            if dx*dx + dy*dy <= 15.0*15.0 { me } else { std::ptr::null_mut() }
        }
    }

    extern "C" fn mouse_down(_: &Object, _: Sel, ev: id) {
        let clicks: i64 = unsafe { msg_send![ev, clickCount] };
        if clicks == 1 && APP_STATE.load(Ordering::SeqCst) == 1 {
            dispatch::Queue::main().exec_async(to_input);
        }
    }

    decl.add_method(sel!(isOpaque),   is_opaque  as extern "C" fn(&Object, Sel) -> BOOL);
    decl.add_method(sel!(drawRect:),  draw_rect  as extern "C" fn(&Object, Sel, NSRect));
    decl.add_method(sel!(hitTest:),   hit_test   as extern "C" fn(&Object, Sel, NSPoint) -> id);
    decl.add_method(sel!(mouseDown:), mouse_down as extern "C" fn(&Object, Sel, id));
    decl.register()
}

// ── InputView ─────────────────────────────────────────────────────────────────

unsafe fn register_input_view() -> *const Class {
    let mut decl = ClassDecl::new("InputView", class!(NSView))
        .expect("InputView already declared");

    extern "C" fn accepts_first_responder(_: &Object, _: Sel) -> BOOL { YES }
    extern "C" fn is_opaque(_: &Object, _: Sel) -> BOOL { NO }

    extern "C" fn key_down(_: &Object, _: Sel, event: id) {
        unsafe {
            let kc: u16 = msg_send![event, keyCode];
            let mods: u64 = msg_send![event, modifierFlags];
            let cmd = mods & (1 << 20) != 0;
            match kc {
                51 => {
                    let upd = { let mut g = INPUT_TEXT.get().unwrap().lock().unwrap(); g.pop(); g.clone() };
                    update_display(&upd);
                }
                36 | 76 => { to_loading(); }
                53       => { to_hidden(); }
                _ if !cmd => {
                    let chars: id = msg_send![event, characters];
                    if chars != nil {
                        let ptr: *const std::os::raw::c_char = msg_send![chars, UTF8String];
                        if !ptr.is_null() {
                            if let Ok(s) = std::ffi::CStr::from_ptr(ptr).to_str() {
                                let p: String = s.chars().filter(|&c| c >= ' ' && c != '\x7f').collect();
                                if !p.is_empty() {
                                    let upd = { let mut g = INPUT_TEXT.get().unwrap().lock().unwrap(); g.push_str(&p); g.clone() };
                                    update_display(&upd);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    decl.add_method(sel!(acceptsFirstResponder), accepts_first_responder as extern "C" fn(&Object, Sel) -> BOOL);
    decl.add_method(sel!(isOpaque), is_opaque as extern "C" fn(&Object, Sel) -> BOOL);
    decl.add_method(sel!(keyDown:), key_down  as extern "C" fn(&Object, Sel, id));
    decl.register()
}

// ── Input panel ───────────────────────────────────────────────────────────────

unsafe fn build_input_panel(ctrl: id) -> id {
    let panel: id = msg_send![class!(NSPanel), alloc];
    let panel: id = msg_send![panel,
        initWithContentRect: NSRect::new(NSPoint::new(0.0,0.0), NSSize::new(IN_W,IN_H))
        styleMask: 0u64 backing: 2u64 defer: NO
    ];
    let _: () = msg_send![panel, setOpaque: NO];
    let clear: id = msg_send![class!(NSColor), clearColor];
    let _: () = msg_send![panel, setBackgroundColor: clear];
    let _: () = msg_send![panel, setMovableByWindowBackground: YES];
    let _: () = msg_send![panel, setLevel: 25_i64];
    let _: () = msg_send![panel, setHasShadow: YES];
    let _: () = msg_send![panel, setFloatingPanel: YES];
    let _: () = msg_send![panel, setCollectionBehavior: 1u64];

    let ve: id = msg_send![class!(NSVisualEffectView), alloc];
    let ve: id = msg_send![ve, initWithFrame: NSRect::new(NSPoint::new(0.0,0.0), NSSize::new(IN_W,IN_H))];
    let _: () = msg_send![ve, setMaterial: 13_i64];
    let _: () = msg_send![ve, setBlendingMode: 0_i64];
    let _: () = msg_send![ve, setState: 1_i64];
    let _: () = msg_send![ve, setWantsLayer: YES];
    let vel: id = msg_send![ve, layer];
    let _: () = msg_send![vel, setCornerRadius: 12.0_f64];
    let _: () = msg_send![vel, setMasksToBounds: YES];
    let _: () = msg_send![panel, setContentView: ve];

    // Drag strip
    let grip: id = msg_send![class!(NSTextField), alloc];
    let grip: id = msg_send![grip, initWithFrame: NSRect::new(NSPoint::new(0.0, IN_H-16.0), NSSize::new(IN_W, 16.0))];
    let _: () = msg_send![grip, setEditable: NO]; let _: () = msg_send![grip, setBezeled: NO];
    let _: () = msg_send![grip, setDrawsBackground: NO]; let _: () = msg_send![grip, setSelectable: NO];
    let _: () = msg_send![grip, setAlignment: 2_i64];
    let _: () = msg_send![grip, setStringValue: NSString::alloc(nil).init_str("⋯")];
    let gf: id = msg_send![class!(NSFont), systemFontOfSize: 10.0_f64];
    let _: () = msg_send![grip, setFont: gf];
    let gc: id = msg_send![class!(NSColor), colorWithRed:1.0 green:1.0 blue:1.0 alpha:0.25_f64];
    let _: () = msg_send![grip, setTextColor: gc];
    let _: () = msg_send![ve, addSubview: grip];

    // Text input box
    let ch = IN_H - 16.0;
    let bh = 32.0_f64;
    let by = (ch - bh) / 2.0;
    let bw = IN_W - 56.0;
    let box_: id = msg_send![class!(NSView), alloc];
    let box_: id = msg_send![box_, initWithFrame: NSRect::new(NSPoint::new(10.0, by), NSSize::new(bw+4.0, bh))];
    let _: () = msg_send![box_, setWantsLayer: YES];
    let bl: id = msg_send![box_, layer];
    let fill: id = msg_send![class!(NSColor), colorWithRed:0.0 green:0.0 blue:0.0 alpha:0.30_f64];
    let cg_fill: id = msg_send![fill, CGColor];
    let _: () = msg_send![bl, setBackgroundColor: cg_fill];
    let _: () = msg_send![bl, setCornerRadius: 7.0_f64];
    let bord: id = msg_send![class!(NSColor), colorWithRed:0.25 green:0.60 blue:1.00 alpha:0.80_f64];
    let cg_bord: id = msg_send![bord, CGColor];
    let _: () = msg_send![bl, setBorderColor: cg_bord];
    let _: () = msg_send![bl, setBorderWidth: 1.5_f64];
    let _: () = msg_send![ve, addSubview: box_];

    let white: id = msg_send![class!(NSColor), whiteColor];

    // Display label
    let df: id = msg_send![class!(NSTextField), alloc];
    let df: id = msg_send![df, initWithFrame: NSRect::new(NSPoint::new(8.0, 4.0), NSSize::new(bw-12.0, bh-8.0))];
    let _: () = msg_send![df, setEditable: NO]; let _: () = msg_send![df, setSelectable: NO];
    let _: () = msg_send![df, setBezeled: NO]; let _: () = msg_send![df, setDrawsBackground: NO];
    let df_font: id = msg_send![class!(NSFont), systemFontOfSize: 14.0_f64];
    let _: () = msg_send![df, setFont: df_font];
    let _: () = msg_send![df, setTextColor: white];
    let _: () = msg_send![df, setStringValue: NSString::alloc(nil).init_str("")];
    let _: () = msg_send![box_, addSubview: df];

    // Placeholder
    let ph: id = msg_send![class!(NSTextField), alloc];
    let ph: id = msg_send![ph, initWithFrame: NSRect::new(NSPoint::new(8.0, 4.0), NSSize::new(bw-12.0, bh-8.0))];
    let _: () = msg_send![ph, setEditable: NO]; let _: () = msg_send![ph, setBezeled: NO];
    let _: () = msg_send![ph, setDrawsBackground: NO]; let _: () = msg_send![ph, setSelectable: NO];
    let ph_font: id = msg_send![class!(NSFont), systemFontOfSize: 14.0_f64];
    let _: () = msg_send![ph, setFont: ph_font];
    let phc: id = msg_send![class!(NSColor), colorWithRed:1.0 green:1.0 blue:1.0 alpha:0.30_f64];
    let _: () = msg_send![ph, setTextColor: phc];
    let _: () = msg_send![ph, setStringValue: NSString::alloc(nil).init_str("Ask anything…")];
    let _: () = msg_send![box_, addSubview: ph];

    // InputView (key capture)
    let iv_cls = register_input_view();
    let iv: id = msg_send![iv_cls, alloc];
    let iv: id = msg_send![iv, initWithFrame: NSRect::new(NSPoint::new(10.0, by), NSSize::new(bw+4.0, bh))];
    let _: () = msg_send![ve, addSubview: iv];

    // Submit button
    let bs = 34.0_f64;
    let btn: id = msg_send![class!(NSButton), alloc];
    let btn: id = msg_send![btn, initWithFrame: NSRect::new(
        NSPoint::new(IN_W - bs - 10.0, (ch - bs) / 2.0), NSSize::new(bs, bs))];
    let _: () = msg_send![btn, setBezelStyle: 0_i64]; let _: () = msg_send![btn, setBordered: NO];
    let _: () = msg_send![btn, setWantsLayer: YES];
    let btnl: id = msg_send![btn, layer];
    let blue: id = msg_send![class!(NSColor), colorWithRed:0.07 green:0.47 blue:1.0 alpha:1.0_f64];
    let cg_blue: id = msg_send![blue, CGColor];
    let _: () = msg_send![btnl, setBackgroundColor: cg_blue];
    let _: () = msg_send![btnl, setCornerRadius: bs/2.0];
    let _: () = msg_send![btn, setTitle: NSString::alloc(nil).init_str("\u{21A9}")];
    let btn_font: id = msg_send![class!(NSFont), systemFontOfSize: 16.0_f64];
    let _: () = msg_send![btn, setFont: btn_font];
    let _: () = msg_send![btn, setContentTintColor: white];
    let _: () = msg_send![btn, setAction: sel!(enterPressed:)];
    let _: () = msg_send![btn, setTarget: ctrl];
    let _: () = msg_send![ve, addSubview: btn];

    DISPLAY_FIELD_PTR.set(df as usize).unwrap();
    PLACEHOLDER_PTR  .set(ph as usize).unwrap();
    INPUT_VIEW_PTR   .set(iv as usize).unwrap();
    panel
}

// ── Step panel ────────────────────────────────────────────────────────────────
//
// Layout (NS coords, y=0 at bottom, STEP_H=172):
//
//  172 ┌──────────────────────────────────────┐
//      │  STEP N  ·  ACTION  (small header)   │  y=154, h=16
//  154 ├──────────────────────────────────────┤  sep y=152
//      │                                      │
//      │  Full instruction text here          │  y=64, h=86 (6 lines @ 13pt)
//      │  wrapping across multiple lines      │
//      │                                      │
//   64 ├──────────────────────────────────────┤  sep y=62
//      │                                      │
//   10 │  [   I did it  →   /  Done ✓   ]     │  y=10, h=46
//    0 └──────────────────────────────────────┘

unsafe fn build_step_panel(ctrl: id) -> id {
    let panel: id = msg_send![class!(NSPanel), alloc];
    let panel: id = msg_send![panel,
        initWithContentRect: NSRect::new(NSPoint::new(0.0,0.0), NSSize::new(STEP_W, STEP_H))
        styleMask: 0u64 backing: 2u64 defer: NO
    ];
    let _: () = msg_send![panel, setOpaque: NO];
    let clear: id = msg_send![class!(NSColor), clearColor];
    let _: () = msg_send![panel, setBackgroundColor: clear];
    let _: () = msg_send![panel, setMovableByWindowBackground: YES];
    let _: () = msg_send![panel, setLevel: 25_i64];
    let _: () = msg_send![panel, setHasShadow: YES];
    let _: () = msg_send![panel, setFloatingPanel: YES];
    let _: () = msg_send![panel, setCollectionBehavior: 1u64];

    let ve: id = msg_send![class!(NSVisualEffectView), alloc];
    let ve: id = msg_send![ve, initWithFrame: NSRect::new(NSPoint::new(0.0,0.0), NSSize::new(STEP_W, STEP_H))];
    let _: () = msg_send![ve, setMaterial: 13_i64];
    let _: () = msg_send![ve, setBlendingMode: 0_i64];
    let _: () = msg_send![ve, setState: 1_i64];
    let _: () = msg_send![ve, setWantsLayer: YES];
    let vel: id = msg_send![ve, layer];
    let _: () = msg_send![vel, setCornerRadius: 14.0_f64];
    let _: () = msg_send![vel, setMasksToBounds: YES];
    let _: () = msg_send![panel, setContentView: ve];

    let white: id = msg_send![class!(NSColor), whiteColor];
    let dim:   id = msg_send![class!(NSColor), colorWithRed:1.0_f64 green:1.0_f64 blue:1.0_f64 alpha:0.55_f64];

    let make_lbl = |frame: NSRect, font: id, color: id, text: &str| -> id {
        let f: id = msg_send![class!(NSTextField), alloc];
        let f: id = msg_send![f, initWithFrame: frame];
        let _: () = msg_send![f, setEditable: NO]; let _: () = msg_send![f, setBezeled: NO];
        let _: () = msg_send![f, setDrawsBackground: NO]; let _: () = msg_send![f, setSelectable: NO];
        let _: () = msg_send![f, setFont: font]; let _: () = msg_send![f, setTextColor: color];
        let _: () = msg_send![f, setStringValue: NSString::alloc(nil).init_str(text)];
        f
    };

    // Header: "STEP N · ACTION"
    let head_font: id = msg_send![class!(NSFont), systemFontOfSize: 11.0_f64];
    let head = make_lbl(
        NSRect::new(NSPoint::new(16.0, 154.0), NSSize::new(STEP_W - 32.0, 16.0)),
        head_font, dim, "",
    );
    let _: () = msg_send![ve, addSubview: head];

    // Separator below header
    let sep1: id = msg_send![class!(NSBox), alloc];
    let sep1: id = msg_send![sep1,
        initWithFrame: NSRect::new(NSPoint::new(16.0, 152.0), NSSize::new(STEP_W - 32.0, 1.0))
    ];
    let _: () = msg_send![sep1, setBoxType: 2_i64];
    let _: () = msg_send![ve, addSubview: sep1];

    // Body: main instruction text (large, word-wrapped)
    let body_font: id = msg_send![class!(NSFont), systemFontOfSize: 14.0_f64];
    let body = make_lbl(
        NSRect::new(NSPoint::new(16.0, 64.0), NSSize::new(STEP_W - 32.0, 86.0)),
        body_font, white, "",
    );
    let _: () = msg_send![body, setLineBreakMode: 6_i64]; // NSLineBreakByWordWrapping
    let _: () = msg_send![body, setMaximumNumberOfLines: 5_i64];
    let body_cell: id = msg_send![body, cell];
    let _: () = msg_send![body_cell, setWraps: YES];
    let _: () = msg_send![body_cell, setScrollable: NO];
    let _: () = msg_send![ve, addSubview: body];

    // Separator above button
    let sep2: id = msg_send![class!(NSBox), alloc];
    let sep2: id = msg_send![sep2,
        initWithFrame: NSRect::new(NSPoint::new(16.0, 62.0), NSSize::new(STEP_W - 32.0, 1.0))
    ];
    let _: () = msg_send![sep2, setBoxType: 2_i64];
    let _: () = msg_send![ve, addSubview: sep2];

    // "I did it →" button
    let bw = STEP_W - 32.0;
    let next_btn: id = msg_send![class!(NSButton), alloc];
    let next_btn: id = msg_send![next_btn,
        initWithFrame: NSRect::new(NSPoint::new(16.0, 10.0), NSSize::new(bw, 46.0))
    ];
    let _: () = msg_send![next_btn, setBezelStyle: 0_i64];
    let _: () = msg_send![next_btn, setBordered: NO];
    let _: () = msg_send![next_btn, setWantsLayer: YES];
    let btnl: id = msg_send![next_btn, layer];
    let blue: id = msg_send![class!(NSColor), colorWithRed:0.07 green:0.47 blue:1.0 alpha:1.0_f64];
    let cg_blue2: id = msg_send![blue, CGColor];
    let _: () = msg_send![btnl, setBackgroundColor: cg_blue2];
    let _: () = msg_send![btnl, setCornerRadius: 12.0_f64];
    let _: () = msg_send![next_btn, setTitle: NSString::alloc(nil).init_str("I did it  \u{2192}")];
    let btnf: id = msg_send![class!(NSFont), boldSystemFontOfSize: 14.0_f64];
    let _: () = msg_send![next_btn, setFont: btnf];
    let _: () = msg_send![next_btn, setContentTintColor: white];
    let _: () = msg_send![next_btn, setAction: sel!(nextStepPressed:)];
    let _: () = msg_send![next_btn, setTarget: ctrl];
    let _: () = msg_send![ve, addSubview: next_btn];

    STEP_HEAD_PTR    .set(head     as usize).unwrap();
    STEP_BODY_PTR    .set(body     as usize).unwrap();
    STEP_NEXT_BTN_PTR.set(next_btn as usize).unwrap();
    panel
}

// ── Accessibility ─────────────────────────────────────────────────────────────

unsafe fn ensure_accessibility() {
    extern "C" {
        fn AXIsProcessTrusted() -> BOOL;
        fn AXIsProcessTrustedWithOptions(options: id) -> BOOL;
    }
    if AXIsProcessTrusted() == YES { return; }
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    eprintln!("\n[pointer] Accessibility not granted.\n  Add: {exe}\n  System Settings → Privacy & Security → Accessibility\n");
    let key: id = NSString::alloc(nil).init_str("AXTrustedCheckOptionPrompt");
    let val: id = msg_send![class!(NSNumber), numberWithBool: YES];
    let opts: id = msg_send![class!(NSDictionary), dictionaryWithObject:val forKey:key];
    AXIsProcessTrustedWithOptions(opts);
}

// ── CGEventTap ────────────────────────────────────────────────────────────────

const VK_ANSI_P:   u16 = 35;
const VK_DELETE:   u16 = 51;
const VK_RETURN:   u16 = 36;
const VK_KP_ENTER: u16 = 76;
const VK_ESCAPE:   u16 = 53;
const CGE_KEY_DOWN: u32 = 10;
const CGE_KEY_UP:   u32 = 11;
const CGE_TAP_DISABLED_BY_TIMEOUT:    u32 = 0xFFFFFFFE;
const CGE_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFFFFFF;
const CGE_LEFT_MOUSE_DOWN:  u32 = 1;
const MASK_LEFT_MOUSE_DOWN: u64 = 1 << 1;
const MASK_KEY_DOWN: u64 = 1 << 10;
const MASK_KEY_UP:   u64 = 1 << 11;
const NX_COMMAND_MASK: u64 = 0x0010_0000;
const NX_SHIFT_MASK:   u64 = 0x0002_0000;
const KCG_KEYBOARD_EVENT_KEYCODE: i32 = 9;

extern "C" {
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, eventsOfInterest: u64,
        callback: unsafe extern "C" fn(*const c_void, u32, *const c_void, *const c_void) -> *const c_void,
        userInfo: *const c_void) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetIntegerValueField(event: *const c_void, field: i32) -> i64;
    fn CGEventGetFlags(event: *const c_void) -> u64;
    fn CGEventGetLocation(event: *const c_void) -> NSPoint;
    fn CGEventKeyboardGetUnicodeString(event: *const c_void, maxLen: usize, actualLen: *mut usize, buf: *mut u16);
    fn CFMachPortCreateRunLoopSource(allocator: *const c_void, port: *mut c_void, order: i32) -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopRun();
    static kCFRunLoopDefaultMode: *const c_void;
}

fn process_key(keycode: u16, chars: String) {
    match keycode {
        VK_DELETE => {
            let upd = INPUT_TEXT.get().and_then(|m| m.lock().ok())
                .map(|mut g| { g.pop(); g.clone() });
            if let Some(s) = upd { update_display(&s); }
        }
        VK_RETURN | VK_KP_ENTER => to_loading(),
        VK_ESCAPE               => to_hidden(),
        _ => {
            let p: String = chars.chars().filter(|&c| c >= ' ' && c != '\x7f').collect();
            if !p.is_empty() {
                let upd = INPUT_TEXT.get().and_then(|m| m.lock().ok())
                    .map(|mut g| { g.push_str(&p); g.clone() });
                if let Some(s) = upd { update_display(&s); }
            }
        }
    }
}

unsafe extern "C" fn event_tap_cb(
    _: *const c_void, ev_type: u32, event: *const c_void, _: *const c_void,
) -> *const c_void {
    if ev_type == CGE_TAP_DISABLED_BY_TIMEOUT || ev_type == CGE_TAP_DISABLED_BY_USER_INPUT {
        let tap = EVENT_TAP.load(Ordering::SeqCst) as *mut c_void;
        if !tap.is_null() { CGEventTapEnable(tap, true); }
        return event;
    }
    if ev_type == CGE_LEFT_MOUSE_DOWN {
        if APP_STATE.load(Ordering::SeqCst) == 4
            && STEP_ACTION_CLICK.load(Ordering::SeqCst)
        {
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
        if state == 2 {
            let mut buf = [0u16; 4]; let mut len: usize = 0;
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
    if state == 2 && ev_type == CGE_KEY_UP { return std::ptr::null(); }
    event
}

unsafe fn run_event_tap() {
    loop {
        let tap = CGEventTapCreate(0, 0, 0, MASK_LEFT_MOUSE_DOWN | MASK_KEY_DOWN | MASK_KEY_UP, event_tap_cb, std::ptr::null());
        if tap.is_null() {
            eprintln!("[pointer] Event tap failed — grant Accessibility permission. Retrying in 2s…");
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

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    unsafe {
        let _pool = NSAutoreleasePool::new(nil);
        let app   = NSApp();
        app.setActivationPolicy_(NSApplicationActivationPolicyAccessory);
        let menu: id = msg_send![class!(NSMenu), new];
        let _: () = msg_send![app, setMainMenu: menu];

        ensure_accessibility();

        INPUT_TEXT  .set(Mutex::new(String::new())).unwrap();
        LAST_CONTEXT.set(Mutex::new((String::new(), String::new()))).unwrap();
        TOUR_AGENT  .set(Mutex::new(None)).unwrap();
        GOAL_TEXT   .set(Mutex::new(String::new())).unwrap();

        let view_class = register_pointer_view();

        // ── AppController ─────────────────────────────────────────────────────
        let mut ctrl_decl = ClassDecl::new("AppController", class!(NSObject))
            .expect("AppController already declared");

        extern "C" fn pulse(_: &Object, _: Sel, _: id) {
            let state = APP_STATE.load(Ordering::SeqCst);
            match state {
                1 | 4 => {
                    let tick = BLINK_TICK.fetch_add(1, Ordering::SeqCst);
                    let s = ((tick as f32) * 0.21).sin();
                    BLINK_ALPHA.store((0.22_f32 + 0.71_f32 * s * s).to_bits(), Ordering::SeqCst);
                    if state == 4 {
                        const ANIM_TICKS: u32 = 16; // 800 ms at 50 ms/tick
                        let anim_tick = ANIM_TICK.load(Ordering::SeqCst);
                        if anim_tick <= ANIM_TICKS {
                            ANIM_TICK.fetch_add(1, Ordering::SeqCst);
                            let t = ((anim_tick as f64) / (ANIM_TICKS as f64)).min(1.0);
                            let t = t * t * (3.0 - 2.0 * t); // smoothstep
                            let fx = f64::from_bits(ANIM_FROM_X.load(Ordering::SeqCst));
                            let fy = f64::from_bits(ANIM_FROM_Y.load(Ordering::SeqCst));
                            let tx = f64::from_bits(ANIM_TO_X.load(Ordering::SeqCst));
                            let ty = f64::from_bits(ANIM_TO_Y.load(Ordering::SeqCst));
                            let cx = fx + (tx - fx) * t;
                            let cy = fy + (ty - fy) * t;
                            unsafe {
                                let _: () = msg_send![dot_win(), setFrame:
                                    NSRect::new(NSPoint::new(cx - DOT_SZ/2.0, cy - DOT_SZ/2.0),
                                                NSSize::new(DOT_SZ, DOT_SZ))
                                    display: NO
                                ];
                            }
                        }
                    }
                    // Countdown to completion panel after final-step flash.
                    if state == 4 {
                        let cd = COMPLETE_COUNTDOWN.load(Ordering::SeqCst);
                        if cd > 0 {
                            COMPLETE_COUNTDOWN.fetch_sub(1, Ordering::SeqCst);
                        } else if cd == 0 {
                            COMPLETE_COUNTDOWN.store(-1, Ordering::SeqCst);
                            unsafe { show_completion(); }
                        }
                    }
                    unsafe { redraw_dot(); }
                }
                2 => {
                    let tick = CURSOR_TICK.fetch_add(1, Ordering::SeqCst);
                    if tick % 10 == 0 {
                        CURSOR_ON.store((tick / 10) % 2 == 0, Ordering::SeqCst);
                        if let Some(m) = INPUT_TEXT.get() {
                            if let Ok(g) = m.try_lock() { update_display(&g); }
                        }
                    }
                }
                3 => {
                    LOAD_TICK.fetch_add(1, Ordering::SeqCst);
                    unsafe { redraw_dot(); }
                }
                5 => {
                    // Auto-dismiss completion panel after ~5 s.
                    let tick = DONE_TICK.fetch_add(1, Ordering::SeqCst);
                    if tick >= 100 { to_hidden(); }
                }
                _ => {}
            }
        }

        extern "C" fn enter_pressed(_: &Object, _: Sel, _: id) {
            dispatch::Queue::main().exec_async(to_loading);
        }

        extern "C" fn next_step_pressed(_: &Object, _: Sel, _: id) {
            dispatch::Queue::main().exec_async(on_next_step_pressed);
        }

        ctrl_decl.add_method(sel!(pulse:),           pulse            as extern "C" fn(&Object, Sel, id));
        ctrl_decl.add_method(sel!(enterPressed:),    enter_pressed    as extern "C" fn(&Object, Sel, id));
        ctrl_decl.add_method(sel!(nextStepPressed:), next_step_pressed as extern "C" fn(&Object, Sel, id));
        let ctrl_class = ctrl_decl.register();
        let ctrl: id = msg_send![ctrl_class, alloc];
        let ctrl: id = msg_send![ctrl, init];

        // ── Dot window ────────────────────────────────────────────────────────
        let dot_win: id = NSWindow::alloc(nil).initWithContentRect_styleMask_backing_defer_(
            NSRect::new(NSPoint::new(0.0,0.0), NSSize::new(DOT_SZ, DOT_SZ)),
            NSWindowStyleMask::NSBorderlessWindowMask,
            NSBackingStoreType::NSBackingStoreBuffered, NO,
        );
        let _: () = msg_send![dot_win, setOpaque: NO];
        let dot_clear: id = msg_send![class!(NSColor), clearColor];
        let _: () = msg_send![dot_win, setBackgroundColor: dot_clear];
        let _: () = msg_send![dot_win, setLevel: 25_i64];
        let _: () = msg_send![dot_win, setHasShadow: NO];
        let _: () = msg_send![dot_win, setCollectionBehavior: 1u64];
        let dot_view: id = msg_send![view_class, alloc];
        let dot_view: id = msg_send![dot_view,
            initWithFrame: NSRect::new(NSPoint::new(0.0,0.0), NSSize::new(DOT_SZ, DOT_SZ))
        ];
        let _: () = msg_send![dot_win, setContentView: dot_view];
        DOT_WIN_PTR .set(dot_win  as usize).unwrap();
        DOT_VIEW_PTR.set(dot_view as usize).unwrap();

        // ── Input panel ───────────────────────────────────────────────────────
        let input_panel = build_input_panel(ctrl);
        IN_PANEL_PTR.set(input_panel as usize).unwrap();

        // ── Step panel ────────────────────────────────────────────────────────
        let step_panel = build_step_panel(ctrl);
        STEP_WIN_PTR.set(step_panel as usize).unwrap();

        // ── 50 ms animation timer ─────────────────────────────────────────────
        let _: id = msg_send![class!(NSTimer),
            scheduledTimerWithTimeInterval: 0.05_f64
            target: ctrl selector: sel!(pulse:) userInfo: nil as id repeats: YES
        ];

        thread::spawn(|| run_event_tap());
        app.run();
    }
}
