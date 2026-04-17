#![allow(non_snake_case)]

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
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;

// ── Constants ─────────────────────────────────────────────────────────────────

const DOT_SZ: f64 = 48.0;
const IN_W:   f64 = 260.0;
const IN_H:   f64 = 72.0;

// ── State: 0=hidden 1=dot 2=input 3=loading ───────────────────────────────────

static APP_STATE:   AtomicU8  = AtomicU8::new(0);
static BLINK_TICK:  AtomicU32 = AtomicU32::new(0);
static BLINK_ALPHA: AtomicU32 = AtomicU32::new(0); // f32 bits
static LOAD_TICK:   AtomicU32 = AtomicU32::new(0);
static CURSOR_TICK: AtomicU32 = AtomicU32::new(0);
static CURSOR_ON:   AtomicBool = AtomicBool::new(true);
// Stores the CFMachPortRef of our active CGEventTap so the callback can re-enable it.
static EVENT_TAP:   AtomicUsize = AtomicUsize::new(0);

static DOT_WIN_PTR:       OnceLock<usize> = OnceLock::new();
static DOT_VIEW_PTR:      OnceLock<usize> = OnceLock::new();
static IN_PANEL_PTR:      OnceLock<usize> = OnceLock::new();
static INPUT_VIEW_PTR:    OnceLock<usize> = OnceLock::new(); // custom key-handler view
static DISPLAY_FIELD_PTR: OnceLock<usize> = OnceLock::new();
static PLACEHOLDER_PTR:   OnceLock<usize> = OnceLock::new();
static INPUT_TEXT:        OnceLock<Mutex<String>> = OnceLock::new();
// Snapshot of (app name, window title) taken when the dot appears.
static LAST_CONTEXT: OnceLock<Mutex<(String, String)>> = OnceLock::new();

// ── Accessors ─────────────────────────────────────────────────────────────────

#[inline] unsafe fn dot_win()       -> id { *DOT_WIN_PTR.get().unwrap()       as id }
#[inline] unsafe fn dot_view()      -> id { *DOT_VIEW_PTR.get().unwrap()      as id }
#[inline] unsafe fn in_panel()      -> id { *IN_PANEL_PTR.get().unwrap()      as id }
#[inline] unsafe fn input_view()    -> id { *INPUT_VIEW_PTR.get().unwrap()    as id }
#[inline] unsafe fn display_field() -> id { *DISPLAY_FIELD_PTR.get().unwrap() as id }
#[inline] unsafe fn placeholder()   -> id { *PLACEHOLDER_PTR.get().unwrap()   as id }

#[inline] unsafe fn redraw_dot() {
    let _: () = msg_send![dot_view(), setNeedsDisplay: YES];
}

// Called on the main thread — updates the display label and cursor.
fn update_display(text: &str) {
    unsafe {
        let with_cursor = if APP_STATE.load(Ordering::SeqCst) == 2
            && CURSOR_ON.load(Ordering::SeqCst)
        {
            format!("{}\u{2502}", text) // │ cursor character
        } else {
            text.to_owned()
        };
        let ns_str: id = NSString::alloc(nil).init_str(&with_cursor);
        let _: () = msg_send![display_field(), setStringValue: ns_str];
        let _: () = msg_send![placeholder(), setHidden: if text.is_empty() { NO } else { YES }];
    }
}

// ── State machine ─────────────────────────────────────────────────────────────

fn toggle()     { if APP_STATE.load(Ordering::SeqCst) == 0 { unsafe { to_dot() } } else { to_hidden() } }
fn to_hidden()  { unsafe { to_hidden_impl()  } }
fn to_input()   { unsafe { to_input_impl()   } }
fn to_loading() { unsafe { to_loading_impl() } }

// ── Context capture (app name + window title of the previously-focused app) ──

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

    let ax_app = AXUIElementCreateApplication(pid);
    if ax_app.is_null() { return String::new(); }

    let attr_win: id = NSString::alloc(nil).init_str("AXFocusedWindow");
    let mut win: *const c_void = std::ptr::null();
    if AXUIElementCopyAttributeValue(ax_app, attr_win as _, &mut win) != 0 || win.is_null() {
        return String::new();
    }

    let attr_title: id = NSString::alloc(nil).init_str("AXTitle");
    let mut title_ref: *const c_void = std::ptr::null();
    if AXUIElementCopyAttributeValue(win, attr_title as _, &mut title_ref) != 0 || title_ref.is_null() {
        return String::new();
    }

    let mut buf = [0i8; 512];
    if CFStringGetCString(title_ref, buf.as_mut_ptr(), 512, CF_UTF8) == NO {
        return String::new();
    }
    std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
}

unsafe fn capture_context() {
    let ws: id    = msg_send![class!(NSWorkspace), sharedWorkspace];
    let front: id = msg_send![ws, frontmostApplication];
    let app_name  = if !front.is_null() { ns_str(msg_send![front, localizedName]) } else { String::new() };
    let pid: i32  = if !front.is_null() { msg_send![front, processIdentifier] } else { 0 };
    let win_title = if pid > 0 { focused_window_title(pid) } else { String::new() };

    if let Some(m) = LAST_CONTEXT.get() {
        if let Ok(mut g) = m.lock() { *g = (app_name, win_title); }
    }
}

unsafe fn to_dot() {
    capture_context(); // snapshot frontmost app/window before we appear
    let mouse: NSPoint = msg_send![class!(NSEvent), mouseLocation];
    let _: () = msg_send![dot_win(),
        setFrame: NSRect::new(
            NSPoint::new(mouse.x - DOT_SZ / 2.0, mouse.y - DOT_SZ / 2.0),
            NSSize::new(DOT_SZ, DOT_SZ))
        display: YES
    ];
    BLINK_TICK.store(0, Ordering::SeqCst);
    redraw_dot();
    let _: () = msg_send![dot_win(), orderFrontRegardless];
    APP_STATE.store(1, Ordering::SeqCst);
}

unsafe fn to_hidden_impl() {
    let _: () = msg_send![dot_win(),  orderOut: nil as id];
    let _: () = msg_send![in_panel(), orderOut: nil as id];
    if let Some(m) = INPUT_TEXT.get() { if let Ok(mut g) = m.lock() { g.clear(); } }
    APP_STATE.store(0, Ordering::SeqCst);
}

unsafe fn to_input_impl() {
    // Position panel relative to where the dot was.
    let dot_frame: NSRect = msg_send![dot_win(), frame];
    let screen: id = msg_send![class!(NSScreen), mainScreen];
    let sf: NSRect = msg_send![screen, frame];
    let mut ix = dot_frame.origin.x + (DOT_SZ - IN_W) / 2.0;
    let mut iy = dot_frame.origin.y + DOT_SZ + 6.0;
    ix = ix.max(8.0).min(sf.size.width  - IN_W - 8.0);
    iy = iy.max(8.0).min(sf.size.height - IN_H - 8.0);

    let _: () = msg_send![dot_win(), orderOut: nil as id];
    let _: () = msg_send![in_panel(),
        setFrame: NSRect::new(NSPoint::new(ix, iy), NSSize::new(IN_W, IN_H))
        display: YES
    ];

    // Show cursor immediately.
    CURSOR_TICK.store(0, Ordering::SeqCst);
    CURSOR_ON.store(true, Ordering::SeqCst);
    APP_STATE.store(2, Ordering::SeqCst); // set before update_display reads it
    update_display("");

    // Show the panel. Key events reach us via the CGEventTap (event_tap_cb),
    // which suppresses them and dispatches to process_key on the main thread.
    // We do NOT call activateIgnoringOtherApps: — not needed with an event tap,
    // and it can crash an unbundled Accessory app on macOS 14+.
    let _: () = msg_send![in_panel(), makeKeyAndOrderFront: nil as id];
    let _: () = msg_send![in_panel(), makeFirstResponder: input_view()];
}

unsafe fn to_loading_impl() {
    // Print context + query before clearing.
    let query = INPUT_TEXT.get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_default();
    let (app, win) = LAST_CONTEXT.get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_default();
    println!("app:    {app}");
    println!("window: {win}");
    println!("query:  {query}");

    if let Some(m) = INPUT_TEXT.get() { if let Ok(mut g) = m.lock() { g.clear(); } }
    update_display("");
    let _: () = msg_send![in_panel(), orderOut: nil as id];
    LOAD_TICK.store(0, Ordering::SeqCst);
    APP_STATE.store(3, Ordering::SeqCst);
    let _: () = msg_send![dot_win(), orderFrontRegardless];
    redraw_dot();
}

// ── PointerView (the floating dot) ────────────────────────────────────────────

unsafe fn register_pointer_view() -> *const Class {
    let mut decl = ClassDecl::new("PointerView", class!(NSView))
        .expect("PointerView already declared");

    extern "C" fn is_opaque(_: &Object, _: Sel) -> BOOL { NO }

    extern "C" fn draw_rect(this: &Object, _: Sel, _: NSRect) {
        unsafe {
            let me = this as *const Object as id;
            let b: NSRect = msg_send![me, bounds];
            let cx = b.size.width  / 2.0;
            let cy = b.size.height / 2.0;

            match APP_STATE.load(Ordering::SeqCst) {
                1 => {
                    let a = f32::from_bits(BLINK_ALPHA.load(Ordering::SeqCst)) as f64;
                    let t = (a - 0.22) / 0.71;

                    // Soft glow
                    let gr = 18.0 + 8.0 * t;
                    let glow: id = msg_send![class!(NSBezierPath),
                        bezierPathWithOvalInRect: NSRect::new(
                            NSPoint::new(cx - gr, cy - gr), NSSize::new(gr*2.0, gr*2.0))];
                    let gc: id = msg_send![class!(NSColor),
                        colorWithRed:0.20 green:0.55 blue:1.00 alpha: a*0.28];
                    let _: () = msg_send![gc, set]; let _: () = msg_send![glow, fill];

                    // Outer ring
                    let rr = 11.0 + 2.0 * t;
                    let ring: id = msg_send![class!(NSBezierPath),
                        bezierPathWithOvalInRect: NSRect::new(
                            NSPoint::new(cx - rr, cy - rr), NSSize::new(rr*2.0, rr*2.0))];
                    let _: () = msg_send![ring, setLineWidth: 1.5_f64];
                    let rc: id = msg_send![class!(NSColor),
                        colorWithRed:0.25 green:0.60 blue:1.00 alpha: a*0.75];
                    let _: () = msg_send![rc, set]; let _: () = msg_send![ring, stroke];

                    // Inner core
                    let ir = 5.0 + 1.5 * t;
                    let dot: id = msg_send![class!(NSBezierPath),
                        bezierPathWithOvalInRect: NSRect::new(
                            NSPoint::new(cx - ir, cy - ir), NSSize::new(ir*2.0, ir*2.0))];
                    let dc: id = msg_send![class!(NSColor),
                        colorWithRed:0.10 green:0.50 blue:1.00 alpha: a];
                    let _: () = msg_send![dc, set]; let _: () = msg_send![dot, fill];

                    // Specular highlight
                    let sa = ((a - 0.60) * 2.5).max(0.0).min(1.0) * 0.65;
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
                    // Centre dot
                    let ir = 5.0_f64;
                    let dot: id = msg_send![class!(NSBezierPath),
                        bezierPathWithOvalInRect: NSRect::new(
                            NSPoint::new(cx-ir, cy-ir), NSSize::new(ir*2.0, ir*2.0))];
                    let dc: id = msg_send![class!(NSColor),
                        colorWithRed:0.10 green:0.50 blue:1.00 alpha:0.35];
                    let _: () = msg_send![dc, set]; let _: () = msg_send![dot, fill];
                    // Spinning arc
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

    extern "C" fn hit_test(this: &Object, _: Sel, pt: NSPoint) -> id {
        unsafe {
            let me = this as *const Object as id;
            let lp: NSPoint = msg_send![me, convertPoint: pt fromView: nil as id];
            let b:  NSRect  = msg_send![me, bounds];
            let dx = lp.x - b.size.width / 2.0;
            let dy = lp.y - b.size.height / 2.0;
            if dx*dx + dy*dy <= 16.0*16.0 { me } else { std::ptr::null_mut() }
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

// ── InputView — transparent NSView that handles keyDown: directly ─────────────
//
// By overriding keyDown: we bypass NSTextView, field editor, TSM, and undo
// manager entirely. Just: event → characters → update display label.

unsafe fn register_input_view() -> *const Class {
    let mut decl = ClassDecl::new("InputView", class!(NSView))
        .expect("InputView already declared");

    // Accept first responder so makeFirstResponder: succeeds.
    extern "C" fn accepts_first_responder(_: &Object, _: Sel) -> BOOL { YES }
    extern "C" fn is_opaque(_: &Object, _: Sel) -> BOOL { NO }

    extern "C" fn key_down(_: &Object, _: Sel, event: id) {
        unsafe {
            let key_code: u16 = msg_send![event, keyCode];
            // Check Command modifier (bit 20 in modifierFlags).
            let mods: u64 = msg_send![event, modifierFlags];
            let cmd = mods & (1 << 20) != 0;

            match key_code {
                51 => {
                    // ── Backspace ─────────────────────────────────────────────
                    let updated = {
                        let mut g = INPUT_TEXT.get().unwrap().lock().unwrap();
                        g.pop();
                        g.clone()
                    };
                    update_display(&updated);
                }
                36 | 76 => {
                    // ── Return / KP-Return ────────────────────────────────────
                    to_loading();
                }
                53 => {
                    // ── Escape ────────────────────────────────────────────────
                    to_hidden();
                }
                _ if !cmd => {
                    // ── Printable character ───────────────────────────────────
                    // [event characters] gives the Unicode string for this key,
                    // respecting the current keyboard layout and shift state.
                    let chars: id = msg_send![event, characters];
                    if chars != nil {
                        let ptr: *const std::os::raw::c_char = msg_send![chars, UTF8String];
                        if !ptr.is_null() {
                            if let Ok(s) = std::ffi::CStr::from_ptr(ptr).to_str() {
                                let printable: String = s.chars()
                                    .filter(|&c| c >= ' ' && c != '\x7f')
                                    .collect();
                                if !printable.is_empty() {
                                    let updated = {
                                        let mut g = INPUT_TEXT.get().unwrap().lock().unwrap();
                                        g.push_str(&printable);
                                        g.clone()
                                    };
                                    update_display(&updated);
                                }
                            }
                        }
                    }
                }
                _ => {} // ignore cmd+key combos and other special keys
            }
        }
    }

    decl.add_method(
        sel!(acceptsFirstResponder),
        accepts_first_responder as extern "C" fn(&Object, Sel) -> BOOL,
    );
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
    let _: () = msg_send![panel, setBackgroundColor:           clear];
    let _: () = msg_send![panel, setMovableByWindowBackground: YES];
    let _: () = msg_send![panel, setLevel:                     25_i64];
    let _: () = msg_send![panel, setHasShadow:                 YES];
    let _: () = msg_send![panel, setFloatingPanel:             YES];
    let _: () = msg_send![panel, setCollectionBehavior:        1u64];

    // Dark frosted-glass background
    let ve: id = msg_send![class!(NSVisualEffectView), alloc];
    let ve: id = msg_send![ve,
        initWithFrame: NSRect::new(NSPoint::new(0.0,0.0), NSSize::new(IN_W,IN_H))
    ];
    let _: () = msg_send![ve, setMaterial:    13_i64]; // HUDWindow
    let _: () = msg_send![ve, setBlendingMode: 0_i64]; // behindWindow
    let _: () = msg_send![ve, setState:        1_i64]; // active
    let _: () = msg_send![ve, setWantsLayer:   YES];
    let ve_layer: id = msg_send![ve, layer];
    let _: () = msg_send![ve_layer, setCornerRadius:  12.0_f64];
    let _: () = msg_send![ve_layer, setMasksToBounds:  YES];
    let _: () = msg_send![panel, setContentView: ve];

    // Drag-handle strip
    let grip: id = msg_send![class!(NSTextField), alloc];
    let grip: id = msg_send![grip,
        initWithFrame: NSRect::new(NSPoint::new(0.0, IN_H-16.0), NSSize::new(IN_W, 16.0))
    ];
    let _: () = msg_send![grip, setEditable:        NO];
    let _: () = msg_send![grip, setBezeled:         NO];
    let _: () = msg_send![grip, setDrawsBackground: NO];
    let _: () = msg_send![grip, setSelectable:      NO];
    let _: () = msg_send![grip, setAlignment:       2_i64];
    let _: () = msg_send![grip, setStringValue:     NSString::alloc(nil).init_str("⋯")];
    let grip_f: id = msg_send![class!(NSFont), systemFontOfSize: 10.0_f64];
    let _: () = msg_send![grip, setFont: grip_f];
    let gc: id = msg_send![class!(NSColor),
        colorWithRed:1.0 green:1.0 blue:1.0 alpha:0.25_f64];
    let _: () = msg_send![grip, setTextColor: gc];
    let _: () = msg_send![ve, addSubview: grip];

    // ── Text input area ───────────────────────────────────────────────────
    // Container box with border + tinted background.
    let content_h = IN_H - 16.0;
    let box_h = 32.0_f64;
    let box_y = (content_h - box_h) / 2.0;
    let box_w = IN_W - 56.0;

    let box_: id = msg_send![class!(NSView), alloc];
    let box_: id = msg_send![box_,
        initWithFrame: NSRect::new(NSPoint::new(10.0, box_y), NSSize::new(box_w+4.0, box_h))
    ];
    let _: () = msg_send![box_, setWantsLayer: YES];
    let bl: id = msg_send![box_, layer];
    let fill: id = msg_send![class!(NSColor),
        colorWithRed:0.0 green:0.0 blue:0.0 alpha:0.30_f64];
    let cg_fill: id = msg_send![fill, CGColor];
    let _: () = msg_send![bl, setBackgroundColor: cg_fill];
    let _: () = msg_send![bl, setCornerRadius:   7.0_f64];
    let bord: id = msg_send![class!(NSColor),
        colorWithRed:0.25 green:0.60 blue:1.00 alpha:0.80_f64];
    let cg_bord: id = msg_send![bord, CGColor];
    let _: () = msg_send![bl, setBorderColor: cg_bord];
    let _: () = msg_send![bl, setBorderWidth: 1.5_f64];
    let _: () = msg_send![ve, addSubview: box_];

    // Non-editable display label (text typed via keyDown: appears here).
    let df: id = msg_send![class!(NSTextField), alloc];
    let df: id = msg_send![df,
        initWithFrame: NSRect::new(NSPoint::new(8.0, 4.0), NSSize::new(box_w-12.0, box_h-8.0))
    ];
    let _: () = msg_send![df, setEditable:        NO];
    let _: () = msg_send![df, setSelectable:      NO];
    let _: () = msg_send![df, setBezeled:         NO];
    let _: () = msg_send![df, setDrawsBackground: NO];
    let df_font: id = msg_send![class!(NSFont), systemFontOfSize: 14.0_f64];
    let _: () = msg_send![df, setFont: df_font];
    let white: id = msg_send![class!(NSColor), whiteColor];
    let _: () = msg_send![df, setTextColor:  white];
    let _: () = msg_send![df, setStringValue: NSString::alloc(nil).init_str("")];
    let _: () = msg_send![box_, addSubview: df];

    // Placeholder label (hidden when text is non-empty).
    let ph: id = msg_send![class!(NSTextField), alloc];
    let ph: id = msg_send![ph,
        initWithFrame: NSRect::new(NSPoint::new(8.0, 4.0), NSSize::new(box_w-12.0, box_h-8.0))
    ];
    let _: () = msg_send![ph, setEditable:        NO];
    let _: () = msg_send![ph, setBezeled:         NO];
    let _: () = msg_send![ph, setDrawsBackground: NO];
    let _: () = msg_send![ph, setSelectable:      NO];
    let ph_font: id = msg_send![class!(NSFont), systemFontOfSize: 14.0_f64];
    let _: () = msg_send![ph, setFont: ph_font];
    let ph_c: id = msg_send![class!(NSColor),
        colorWithRed:1.0 green:1.0 blue:1.0 alpha:0.30_f64];
    let _: () = msg_send![ph, setTextColor:  ph_c];
    let _: () = msg_send![ph, setStringValue: NSString::alloc(nil).init_str("Type here…")];
    let _: () = msg_send![box_, addSubview: ph];

    // Transparent InputView sits on top of box_ and captures all key events.
    // It has no visible appearance — its sole job is acceptsFirstResponder + keyDown:.
    let iv_class = register_input_view();
    let iv: id = msg_send![iv_class, alloc];
    let iv: id = msg_send![iv,
        initWithFrame: NSRect::new(NSPoint::new(10.0, box_y), NSSize::new(box_w+4.0, box_h))
    ];
    let _: () = msg_send![ve, addSubview: iv];

    // ── ↩ round blue submit button ────────────────────────────────────────
    let bs = 34.0_f64;
    let btn: id = msg_send![class!(NSButton), alloc];
    let btn: id = msg_send![btn,
        initWithFrame: NSRect::new(
            NSPoint::new(IN_W - bs - 10.0, (content_h - bs) / 2.0),
            NSSize::new(bs, bs))
    ];
    let _: () = msg_send![btn, setBezelStyle: 0_i64];
    let _: () = msg_send![btn, setBordered:   NO];
    let _: () = msg_send![btn, setWantsLayer: YES];
    let blue_ns: id = msg_send![class!(NSColor),
        colorWithRed:0.07 green:0.47 blue:1.0 alpha:1.0_f64];
    let btn_layer: id = msg_send![btn, layer];
    let cg_blue: id = msg_send![blue_ns, CGColor];
    let _: () = msg_send![btn_layer, setBackgroundColor: cg_blue];
    let _: () = msg_send![btn_layer, setCornerRadius: bs/2.0];
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

// ── Accessibility permission check ───────────────────────────────────────────
//
// grab() requires the *binary itself* (not Terminal) to be trusted.
// Terminal's Accessibility grant does NOT extend to child processes on macOS.
// AXIsProcessTrustedWithOptions with the Prompt option shows the native dialog.

unsafe fn ensure_accessibility() {
    extern "C" {
        fn AXIsProcessTrusted() -> BOOL;
        fn AXIsProcessTrustedWithOptions(options: id) -> BOOL;
    }

    if AXIsProcessTrusted() == YES {
        return;
    }

    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "<unknown>".into());
    eprintln!(
        "\n[pointer] Accessibility not granted for this binary.\
         \n  Binary path : {exe}\
         \n  Fix         : System Settings → Privacy & Security → Accessibility\
         \n                click +  and add the path above.\
         \n  NOTE: adding Terminal is NOT enough — this binary must be trusted.\n"
    );

    // Trigger the native \"wants to control your computer\" system dialog.
    let key: id = NSString::alloc(nil).init_str("AXTrustedCheckOptionPrompt");
    let val: id = msg_send![class!(NSNumber), numberWithBool: YES];
    let opts: id = msg_send![class!(NSDictionary), dictionaryWithObject:val forKey:key];
    AXIsProcessTrustedWithOptions(opts);
}

// ── Raw CGEventTap (replaces rdev) ───────────────────────────────────────────
//
// Virtual key codes (Carbon / HIToolbox)
const VK_ANSI_P:     u16 = 35;
const VK_DELETE:     u16 = 51;
const VK_RETURN:     u16 = 36;
const VK_KP_ENTER:   u16 = 76;
const VK_ESCAPE:     u16 = 53;
// CGEventType values
const CGE_KEY_DOWN:  u32 = 10;
const CGE_KEY_UP:    u32 = 11;
const CGE_TAP_DISABLED_BY_TIMEOUT:    u32 = 0xFFFFFFFE;
const CGE_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFFFFFF;
// CGEventMask bits
const MASK_KEY_DOWN: u64 = 1 << 10;
const MASK_KEY_UP:   u64 = 1 << 11;
// CGEventFlags (modifier bits)
const NX_COMMAND_MASK: u64 = 0x0010_0000; // bit 20
const NX_SHIFT_MASK:   u64 = 0x0002_0000; // bit 17
// CGEventField
const KCG_KEYBOARD_EVENT_KEYCODE: i32 = 9;

extern "C" {
    fn CGEventTapCreate(
        tap:              u32,
        place:            u32,
        options:          u32,
        eventsOfInterest: u64,
        callback: unsafe extern "C" fn(*const c_void, u32, *const c_void, *const c_void) -> *const c_void,
        userInfo:         *const c_void,
    ) -> *mut c_void; // CFMachPortRef
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetIntegerValueField(event: *const c_void, field: i32) -> i64;
    fn CGEventGetFlags(event: *const c_void) -> u64;
    fn CGEventKeyboardGetUnicodeString(
        event:             *const c_void,
        maxStringLength:   usize,
        actualStringLength: *mut usize,
        unicodeString:     *mut u16,
    );
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port:      *mut c_void,
        order:     i32,
    ) -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopRun();
    static kCFRunLoopDefaultMode: *const c_void;
}

// Runs on the main thread, dispatched from the event tap callback.
fn process_key(keycode: u16, chars: String) {
    match keycode {
        VK_DELETE => {
            let updated = INPUT_TEXT.get()
                .and_then(|m| m.lock().ok())
                .map(|mut g| { g.pop(); g.clone() });
            if let Some(s) = updated { update_display(&s); }
        }
        VK_RETURN | VK_KP_ENTER => to_loading(),
        VK_ESCAPE               => to_hidden(),
        _ => {
            let printable: String = chars.chars()
                .filter(|&c| c >= ' ' && c != '\x7f')
                .collect();
            if !printable.is_empty() {
                let updated = INPUT_TEXT.get()
                    .and_then(|m| m.lock().ok())
                    .map(|mut g| { g.push_str(&printable); g.clone() });
                if let Some(s) = updated { update_display(&s); }
            }
        }
    }
}

// CGEventTap callback — runs on the tap's background CFRunLoop thread.
// SAFETY: only calls thread-safe CG/CF primitives and dispatch_async.
unsafe extern "C" fn event_tap_cb(
    _proxy:    *const c_void,
    ev_type:   u32,
    event:     *const c_void,
    _userinfo: *const c_void,
) -> *const c_void {
    // Re-enable tap if macOS disabled it (timeout or user-input revocation).
    if ev_type == CGE_TAP_DISABLED_BY_TIMEOUT || ev_type == CGE_TAP_DISABLED_BY_USER_INPUT {
        let tap = EVENT_TAP.load(Ordering::SeqCst) as *mut c_void;
        if !tap.is_null() { CGEventTapEnable(tap, true); }
        return event;
    }

    if ev_type != CGE_KEY_DOWN && ev_type != CGE_KEY_UP { return event; }

    let keycode = CGEventGetIntegerValueField(event, KCG_KEYBOARD_EVENT_KEYCODE) as u16;
    let flags   = CGEventGetFlags(event);
    let cmd     = flags & NX_COMMAND_MASK != 0;
    let shift   = flags & NX_SHIFT_MASK   != 0;
    let state   = APP_STATE.load(Ordering::SeqCst);

    if ev_type == CGE_KEY_DOWN {
        // ── Cmd+Shift+P → toggle (suppress) ──────────────────────────────
        if keycode == VK_ANSI_P && cmd && shift {
            dispatch::Queue::main().exec_async(toggle);
            return std::ptr::null();
        }

        // ── Input panel: process key ourselves, suppress from everything else
        if state == 2 {
            let mut buf = [0u16; 4];
            let mut len: usize = 0;
            CGEventKeyboardGetUnicodeString(event, buf.len(), &mut len, buf.as_mut_ptr());
            let ch = String::from_utf16_lossy(&buf[..len]).to_owned();
            let kc = keycode;
            dispatch::Queue::main().exec_async(move || process_key(kc, ch));
            return std::ptr::null(); // suppress
        }

        // ── Escape dismisses dot / loading ────────────────────────────────
        if keycode == VK_ESCAPE && (state == 1 || state == 3) {
            dispatch::Queue::main().exec_async(to_hidden);
        }
    }

    // Suppress key-up while input panel is open
    if state == 2 && ev_type == CGE_KEY_UP { return std::ptr::null(); }

    event // pass through
}

// Blocks forever; call from a dedicated thread.
unsafe fn run_event_tap() {
    loop {
        let tap = CGEventTapCreate(
            0, // kCGHIDEventTap
            0, // kCGHeadInsertEventTap
            0, // kCGEventTapOptionDefault (active — can suppress)
            MASK_KEY_DOWN | MASK_KEY_UP,
            event_tap_cb,
            std::ptr::null(),
        );
        if tap.is_null() {
            eprintln!(
                "\n[pointer] Event tap failed — Accessibility permission required.\
                 \n  System Settings → Privacy & Security → Accessibility → add this binary.\
                 \n  Retrying in 2 s…\n"
            );
            thread::sleep(std::time::Duration::from_secs(2));
            continue;
        }
        EVENT_TAP.store(tap as usize, Ordering::SeqCst);
        let src = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
        let rl  = CFRunLoopGetCurrent();
        CFRunLoopAddSource(rl, src, kCFRunLoopDefaultMode);
        CFRunLoopRun(); // blocks; returns only if tap is invalidated
        // If we get here the tap was invalidated — re-create it.
        EVENT_TAP.store(0, Ordering::SeqCst);
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    unsafe {
        let _pool = NSAutoreleasePool::new(nil);
        let app = NSApp();
        app.setActivationPolicy_(NSApplicationActivationPolicyAccessory);

        // A minimal empty menu is required to prevent a crash when we later
        // call activateIgnoringOtherApps: (AppKit walks the menu bar on activate).
        let menu: id = msg_send![class!(NSMenu), new];
        let _: () = msg_send![app, setMainMenu: menu];

        ensure_accessibility();

        INPUT_TEXT.set(Mutex::new(String::new())).unwrap();
        LAST_CONTEXT.set(Mutex::new((String::new(), String::new()))).unwrap();

        let view_class = register_pointer_view();

        // AppController: timer + ↩ button.
        let mut ctrl_decl = ClassDecl::new("AppController", class!(NSObject))
            .expect("AppController already declared");

        extern "C" fn pulse(_: &Object, _: Sel, _: id) {
            match APP_STATE.load(Ordering::SeqCst) {
                1 => {
                    let tick  = BLINK_TICK.fetch_add(1, Ordering::SeqCst);
                    let s     = ((tick as f32) * 0.21).sin();
                    BLINK_ALPHA.store((0.22_f32 + 0.71_f32 * s * s).to_bits(), Ordering::SeqCst);
                    unsafe { redraw_dot(); }
                }
                // Blink the text cursor at ~1 Hz (every 10 × 50 ms ticks).
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
                    let tick = LOAD_TICK.fetch_add(1, Ordering::SeqCst);
                    unsafe { redraw_dot(); }
                    if tick >= 49 {
                        LOAD_TICK.store(0, Ordering::SeqCst);
                        BLINK_TICK.store(0, Ordering::SeqCst);
                        APP_STATE.store(1, Ordering::SeqCst);
                    }
                }
                _ => {}
            }
        }

        extern "C" fn enter_pressed(_: &Object, _: Sel, _: id) {
            dispatch::Queue::main().exec_async(to_loading);
        }

        ctrl_decl.add_method(sel!(pulse:),       pulse         as extern "C" fn(&Object, Sel, id));
        ctrl_decl.add_method(sel!(enterPressed:), enter_pressed as extern "C" fn(&Object, Sel, id));
        let ctrl_class = ctrl_decl.register();
        let ctrl: id = msg_send![ctrl_class, alloc];
        let ctrl: id = msg_send![ctrl, init];

        // ── Dot window ────────────────────────────────────────────────────────
        let dot_win: id = NSWindow::alloc(nil).initWithContentRect_styleMask_backing_defer_(
            NSRect::new(NSPoint::new(0.0,0.0), NSSize::new(DOT_SZ, DOT_SZ)),
            NSWindowStyleMask::NSBorderlessWindowMask,
            NSBackingStoreType::NSBackingStoreBuffered,
            NO,
        );
        let _: () = msg_send![dot_win, setOpaque: NO];
        let clear: id = msg_send![class!(NSColor), clearColor];
        let _: () = msg_send![dot_win, setBackgroundColor:    clear];
        let _: () = msg_send![dot_win, setLevel:              25_i64];
        let _: () = msg_send![dot_win, setHasShadow:          NO];
        let _: () = msg_send![dot_win, setCollectionBehavior: 1u64];

        let dot_view: id = msg_send![view_class, alloc];
        let dot_view: id = msg_send![dot_view,
            initWithFrame: NSRect::new(NSPoint::new(0.0,0.0), NSSize::new(DOT_SZ, DOT_SZ))
        ];
        let _: () = msg_send![dot_win, setContentView: dot_view];
        DOT_WIN_PTR .set(dot_win  as usize).unwrap();
        DOT_VIEW_PTR.set(dot_view as usize).unwrap();

        // ── Input panel ───────────────────────────────────────────────────────
        let panel = build_input_panel(ctrl);
        IN_PANEL_PTR.set(panel as usize).unwrap();

        // ── 50 ms animation timer ─────────────────────────────────────────────
        let _: id = msg_send![class!(NSTimer),
            scheduledTimerWithTimeInterval: 0.05_f64
            target:   ctrl
            selector: sel!(pulse:)
            userInfo: nil as id
            repeats:  YES
        ];

        // ── Custom CGEventTap (no rdev) ───────────────────────────────────────
        // rdev 0.5 calls TSMGetInputSourceProperty from its background CFRunLoop
        // thread; on macOS 14 Sonoma that function asserts it must run on the
        // main queue → instant SIGTRAP.  We bypass rdev entirely and drive a
        // raw CGEventTap ourselves.  The callback only uses:
        //   • CGEventGetIntegerValueField  (thread-safe)
        //   • CGEventGetFlags              (thread-safe)
        //   • CGEventKeyboardGetUnicodeString (thread-safe, no TSM)
        //   • dispatch_async to main queue  (thread-safe)
        thread::spawn(|| run_event_tap());

        app.run();
    }
}
