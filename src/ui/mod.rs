pub mod hint;
pub mod input;
pub mod settings;
pub mod step;

pub use hint::build_hint_panel;
pub use input::build_input_panel;
pub use settings::{build_color_panel, build_model_settings_panel};
pub use step::build_step_panel;

use cocoa::base::{id, nil, BOOL, NO, YES};
use cocoa::foundation::{NSPoint, NSRect};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use std::sync::atomic::Ordering;

use crate::state::*;
use crate::tour::{to_input, to_loading, to_hidden};

// ── PointerView (animated dot) ────────────────────────────────────────────────

pub unsafe fn register_pointer_view() -> *const Class {
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
                1 | 4 => {
                    let a = f32::from_bits(BLINK_ALPHA.load(Ordering::SeqCst)) as f64;
                    let t = (a - 0.22) / 0.71;
                    let (cr, cg, cb) = dot_color_rgb();
                    // Glow
                    let gr = 17.0 + 7.0 * t;
                    let glow: id = msg_send![class!(NSBezierPath),
                        bezierPathWithOvalInRect: NSRect::new(
                            NSPoint::new(cx - gr, cy - gr), cocoa::foundation::NSSize::new(gr * 2.0, gr * 2.0))];
                    let gc: id = msg_send![class!(NSColor),
                        colorWithRed:cr green:cg blue:cb alpha: a * 0.28];
                    let _: () = msg_send![gc, set]; let _: () = msg_send![glow, fill];
                    // Ring
                    let rr = 10.0 + 2.0 * t;
                    let ring: id = msg_send![class!(NSBezierPath),
                        bezierPathWithOvalInRect: NSRect::new(
                            NSPoint::new(cx - rr, cy - rr), cocoa::foundation::NSSize::new(rr * 2.0, rr * 2.0))];
                    let _: () = msg_send![ring, setLineWidth: 1.5_f64];
                    let rc: id = msg_send![class!(NSColor),
                        colorWithRed:cr green:cg blue:cb alpha: a * 0.75];
                    let _: () = msg_send![rc, set]; let _: () = msg_send![ring, stroke];
                    // Core
                    let ir = 5.0 + 1.5 * t;
                    let dot: id = msg_send![class!(NSBezierPath),
                        bezierPathWithOvalInRect: NSRect::new(
                            NSPoint::new(cx - ir, cy - ir), cocoa::foundation::NSSize::new(ir * 2.0, ir * 2.0))];
                    let dc: id = msg_send![class!(NSColor),
                        colorWithRed:cr green:cg blue:cb alpha: a];
                    let _: () = msg_send![dc, set]; let _: () = msg_send![dot, fill];
                    // Specular highlight
                    let sa = ((a - 0.60) * 2.5).max(0.0).min(1.0) * 0.60;
                    if sa > 0.01 {
                        let wr = 2.5_f64;
                        let sp: id = msg_send![class!(NSBezierPath),
                            bezierPathWithOvalInRect: NSRect::new(
                                NSPoint::new(cx - wr + 2.0, cy - wr + 2.0),
                                cocoa::foundation::NSSize::new(wr * 2.0, wr * 2.0))];
                        let wc: id = msg_send![class!(NSColor),
                            colorWithRed:1.0 green:1.0 blue:1.0 alpha:sa];
                        let _: () = msg_send![wc, set]; let _: () = msg_send![sp, fill];
                    }
                }
                3 => {
                    let tick = LOAD_TICK.load(Ordering::SeqCst);
                    let (cr, cg, cb) = dot_color_rgb();
                    let ir = 5.0_f64;
                    let dot: id = msg_send![class!(NSBezierPath),
                        bezierPathWithOvalInRect: NSRect::new(
                            NSPoint::new(cx - ir, cy - ir), cocoa::foundation::NSSize::new(ir * 2.0, ir * 2.0))];
                    let dc: id = msg_send![class!(NSColor),
                        colorWithRed:cr green:cg blue:cb alpha:0.35];
                    let _: () = msg_send![dc, set]; let _: () = msg_send![dot, fill];
                    let start = 90.0 - (tick as f64 * 9.0) % 360.0;
                    let arc: id = msg_send![class!(NSBezierPath), bezierPath];
                    let _: () = msg_send![arc,
                        appendBezierPathWithArcWithCenter: NSPoint::new(cx, cy)
                        radius: 13.0_f64 startAngle: start endAngle: start - 270.0 clockwise: YES];
                    let _: () = msg_send![arc, setLineWidth: 2.5_f64];
                    let ac: id = msg_send![class!(NSColor),
                        colorWithRed:cr green:cg blue:cb alpha:0.90];
                    let _: () = msg_send![ac, set]; let _: () = msg_send![arc, stroke];
                }
                _ => {}
            }
        }
    }

    // Only hittable in state 1 (click to open input).
    extern "C" fn hit_test(this: &Object, _: Sel, pt: NSPoint) -> id {
        unsafe {
            if APP_STATE.load(Ordering::SeqCst) != 1 { return std::ptr::null_mut(); }
            let me = this as *const Object as id;
            let lp: NSPoint = msg_send![me, convertPoint: pt fromView: nil as id];
            let b:  NSRect  = msg_send![me, bounds];
            let dx = lp.x - b.size.width  / 2.0;
            let dy = lp.y - b.size.height / 2.0;
            if dx * dx + dy * dy <= 15.0 * 15.0 { me } else { std::ptr::null_mut() }
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

// ── InputView (key capture for the prompt field) ──────────────────────────────

pub unsafe fn register_input_view() -> *const Class {
    let mut decl = ClassDecl::new("InputView", class!(NSView))
        .expect("InputView already declared");

    extern "C" fn accepts_first_responder(_: &Object, _: Sel) -> BOOL { YES }
    extern "C" fn is_opaque(_: &Object, _: Sel) -> BOOL { NO }

    extern "C" fn key_down(_: &Object, _: Sel, event: id) {
        unsafe {
            let kc: u16   = msg_send![event, keyCode];
            let mods: u64 = msg_send![event, modifierFlags];
            let cmd = mods & (1 << 20) != 0;
            match kc {
                51 => {
                    let upd = {
                        let mut g = INPUT_TEXT.get().unwrap().lock().unwrap();
                        g.pop();
                        g.clone()
                    };
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
                                let p: String = s.chars()
                                    .filter(|&c| c >= ' ' && c != '\x7f')
                                    .collect();
                                if !p.is_empty() {
                                    let upd = {
                                        let mut g = INPUT_TEXT.get().unwrap().lock().unwrap();
                                        g.push_str(&p);
                                        g.clone()
                                    };
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

    decl.add_method(
        sel!(acceptsFirstResponder),
        accepts_first_responder as extern "C" fn(&Object, Sel) -> BOOL,
    );
    decl.add_method(sel!(isOpaque), is_opaque as extern "C" fn(&Object, Sel) -> BOOL);
    decl.add_method(sel!(keyDown:), key_down  as extern "C" fn(&Object, Sel, id));
    decl.register()
}
