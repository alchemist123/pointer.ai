use cocoa::base::{id, nil, BOOL, NO, YES};
use cocoa::foundation::{NSPoint, NSRect, NSSize, NSString};
use objc::{class, msg_send, sel, sel_impl};
use std::ffi::c_void;
use std::sync::atomic::Ordering;

use crate::agent::{self, AgentStep, TourAgent};
use crate::state::*;

// ── Public state-machine wrappers ─────────────────────────────────────────────

pub fn toggle() {
    if APP_STATE.load(Ordering::SeqCst) == 0 {
        unsafe { to_dot() }
    } else {
        to_hidden()
    }
}
pub fn to_hidden()  { unsafe { to_hidden_impl()  } }
pub fn to_input()   { unsafe { to_input_impl()   } }
pub fn to_loading() { unsafe { to_loading_impl() } }

// ── Context capture ───────────────────────────────────────────────────────────

pub unsafe fn focused_window_title(pid: i32) -> String {
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
    if CFStringGetCString(tref, buf.as_mut_ptr(), 512, CF_UTF8) == NO {
        return String::new();
    }
    std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
}

pub unsafe fn capture_context() {
    let ws: id    = msg_send![class!(NSWorkspace), sharedWorkspace];
    let front: id = msg_send![ws, frontmostApplication];
    let app = if !front.is_null() {
        ns_str(msg_send![front, localizedName])
    } else {
        String::new()
    };
    let pid: i32 = if !front.is_null() { msg_send![front, processIdentifier] } else { 0 };
    let win = if pid > 0 { focused_window_title(pid) } else { String::new() };
    if let Some(m) = LAST_CONTEXT.get() {
        if let Ok(mut g) = m.lock() { *g = (app, win); }
    }
}

// ── State transitions ─────────────────────────────────────────────────────────

pub unsafe fn to_dot() {
    capture_context();
    let mouse: NSPoint = msg_send![class!(NSEvent), mouseLocation];
    let _: () = msg_send![dot_win(),
        setFrame: NSRect::new(
            NSPoint::new(mouse.x - DOT_SZ / 2.0, mouse.y - DOT_SZ / 2.0),
            NSSize::new(DOT_SZ, DOT_SZ))
        display: YES
    ];
    let _: () = msg_send![dot_win(), setIgnoresMouseEvents: NO];
    BLINK_TICK.store(0, Ordering::SeqCst);
    redraw_dot();
    let _: () = msg_send![dot_win(), orderFrontRegardless];
    APP_STATE.store(1, Ordering::SeqCst);
}

pub unsafe fn to_hidden_impl() {
    let _: () = msg_send![dot_win(),  orderOut: nil as id];
    let _: () = msg_send![in_panel(), orderOut: nil as id];
    let _: () = msg_send![step_win(), orderOut: nil as id];
    if HINT_WIN_PTR.get().is_some()  { let _: () = msg_send![hint_win(),  orderOut: nil as id]; }
    if COLOR_WIN_PTR.get().is_some() { let _: () = msg_send![color_win(), orderOut: nil as id]; }
    if MODEL_WIN_PTR.get().is_some() { let _: () = msg_send![model_win(), orderOut: nil as id]; }
    let _: () = msg_send![dot_win(), setIgnoresMouseEvents: NO];
    if let Some(m) = INPUT_TEXT.get()    { if let Ok(mut g) = m.lock() { g.clear(); } }
    if let Some(m) = TOUR_AGENT.get()   { if let Ok(mut g) = m.lock() { *g = None; } }
    if let Some(m) = FINAL_ANSWER.get() { if let Ok(mut g) = m.lock() { g.clear(); } }
    APP_STATE.store(0, Ordering::SeqCst);
}

pub unsafe fn to_input_impl() {
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

pub unsafe fn to_loading_impl() {
    let query = INPUT_TEXT.get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_default();
    let (app, win) = LAST_CONTEXT.get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_default();
    let url = agent::get_browser_url(&app);

    println!("app:    {app}");
    println!("window: {win}");
    if let Some(ref u) = url { println!("url:    {u}"); }
    println!("query:  {query}");

    if let Some(m) = GOAL_TEXT.get()  { if let Ok(mut g) = m.lock() { *g = query.clone(); } }
    if let Some(m) = INPUT_TEXT.get() { if let Ok(mut g) = m.lock() { g.clear(); } }
    update_display("");
    let _: () = msg_send![in_panel(), orderOut: nil as id];
    let _: () = msg_send![step_win(), orderOut: nil as id];

    LOAD_TICK.store(0, Ordering::SeqCst);
    APP_STATE.store(3, Ordering::SeqCst);
    let _: () = msg_send![dot_win(), orderFrontRegardless];
    redraw_dot();

    let cfg = POINTER_CONFIG.get().unwrap().lock().unwrap().clone();
    if cfg.effective_api_key().is_empty() {
        APP_STATE.store(0, Ordering::SeqCst);
        let _: () = msg_send![in_panel(), orderOut: nil as id];
        if MODEL_WIN_PTR.get().is_some() {
            let _: () = msg_send![model_win(), center];
            let _: () = msg_send![model_win(), makeKeyAndOrderFront: nil as id];
        }
        return;
    }

    let screens: id = msg_send![class!(NSScreen), screens];
    let primary: id = msg_send![screens, objectAtIndex: 0usize];
    let sf: NSRect  = msg_send![primary, frame];

    SCREEN_H_LOGICAL.store(sf.size.height.to_bits(), Ordering::SeqCst);
    let new_agent = TourAgent::new(
        query, app, win, url,
        sf.size.width, sf.size.height,
        cfg.effective_api_key().to_owned(),
        cfg.effective_api_url(),
        cfg.effective_model().to_owned(),
    );
    if let Some(m) = TOUR_AGENT.get() {
        if let Ok(mut g) = m.lock() { *g = Some(new_agent); }
    }
    fire_next_step();
}

// ── Tour-guide step loop ──────────────────────────────────────────────────────

pub fn fire_next_step() {
    let agent = match TOUR_AGENT.get().and_then(|m| m.lock().ok()) {
        Some(mut g) => match g.take() {
            Some(a) => a,
            None => {
                eprintln!("[pointer] fire_next_step: agent is None — aborting");
                to_hidden();
                return;
            }
        },
        None => {
            eprintln!("[pointer] fire_next_step: TOUR_AGENT not initialised");
            return;
        }
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

pub fn show_step(step: AgentStep) {
    unsafe {
        let curr: NSRect = msg_send![dot_win(), frame];
        let from_x = curr.origin.x + DOT_SZ / 2.0;
        let from_y = curr.origin.y + DOT_SZ / 2.0;
        ANIM_FROM_X.store(from_x.to_bits(), Ordering::SeqCst);
        ANIM_FROM_Y.store(from_y.to_bits(), Ordering::SeqCst);
        ANIM_TO_X.store(step.x.to_bits(), Ordering::SeqCst);
        ANIM_TO_Y.store(step.y.to_bits(), Ordering::SeqCst);
        ANIM_TICK.store(0, Ordering::SeqCst);

        let sh = f64::from_bits(SCREEN_H_LOGICAL.load(Ordering::SeqCst));
        STEP_TARGET_CG_X.store(step.x.to_bits(), Ordering::SeqCst);
        STEP_TARGET_CG_Y.store((sh - step.y).to_bits(), Ordering::SeqCst);
        let _: () = msg_send![dot_win(), setIgnoresMouseEvents: YES];

        let is_click = matches!(step.action.as_str(), "Click" | "Double-click" | "Right-click");
        STEP_ACTION_CLICK.store(is_click, Ordering::SeqCst);

        let head: id = *STEP_HEAD_PTR.get().unwrap() as id;
        let body: id = *STEP_BODY_PTR.get().unwrap() as id;
        let btn:  id = *STEP_NEXT_BTN_PTR.get().unwrap() as id;

        let head_txt = format!("Step {}  ·  {}", step.step_num, step.action.to_uppercase());
        let _: () = msg_send![head, setStringValue: NSString::alloc(nil).init_str(&head_txt)];
        let _: () = msg_send![body, setStringValue: NSString::alloc(nil).init_str(&step.description)];
        let reason_lbl: id = *STEP_REASON_PTR.get().unwrap() as id;
        let _: () = msg_send![reason_lbl, setStringValue: NSString::alloc(nil).init_str(&step.reason)];

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

        let screen: id = msg_send![class!(NSScreen), mainScreen];
        let sf: NSRect  = msg_send![screen, frame];
        let px = (step.x - STEP_W / 2.0).max(8.0).min(sf.size.width  - STEP_W - 8.0);
        let py_above = step.y + DOT_SZ / 2.0 + 14.0;
        let py_below = step.y - DOT_SZ / 2.0 - STEP_H - 14.0;
        let py = if py_above + STEP_H < sf.size.height - 8.0 { py_above } else { py_below.max(8.0) };
        let _: () = msg_send![step_win(),
            setFrame: NSRect::new(NSPoint::new(px, py), NSSize::new(STEP_W, STEP_H))
            display: YES
        ];

        let hint_text = match step.action.as_str() {
            "Click"        => "👆  Click here",
            "Double-click" => "👆  Double-click here",
            "Right-click"  => "🖱  Right-click here",
            "Type"         => "⌨  Type here",
            "Scroll"       => "↕  Scroll here",
            "Hover"        => "👀  Hover here",
            _              => "↗  Do this",
        };
        let hlbl: id = *HINT_LBL_PTR.get().unwrap() as id;
        let _: () = msg_send![hlbl, setStringValue: NSString::alloc(nil).init_str(hint_text)];
        let hx = (step.x - HINT_W / 2.0).max(8.0).min(sf.size.width - HINT_W - 8.0);
        let hy = (step.y - DOT_SZ / 2.0 - HINT_H - 6.0).max(8.0);
        let _: () = msg_send![hint_win(),
            setFrame: NSRect::new(NSPoint::new(hx, hy), NSSize::new(HINT_W, HINT_H))
            display: YES
        ];

        STEP_IS_FINAL.store(step.is_final, Ordering::SeqCst);
        if step.is_final {
            if let Some(m) = FINAL_ANSWER.get() {
                if let Ok(mut g) = m.lock() { *g = step.description.clone(); }
            }
        }
        BLINK_TICK.store(0, Ordering::SeqCst);
        APP_STATE.store(4, Ordering::SeqCst);
        let _: () = msg_send![step_win(), orderFrontRegardless];
        let _: () = msg_send![hint_win(), orderFrontRegardless];
        let _: () = msg_send![dot_win(),  orderFrontRegardless];
        redraw_dot();
    }
}

pub fn on_next_step_pressed() {
    let state = APP_STATE.load(Ordering::SeqCst);
    if state == 5 { to_hidden(); return; }

    if STEP_IS_FINAL.load(Ordering::SeqCst) {
        if COMPLETE_COUNTDOWN.load(Ordering::SeqCst) >= 0 { return; }
        unsafe {
            let head: id = *STEP_HEAD_PTR.get().unwrap() as id;
            let body: id = *STEP_BODY_PTR.get().unwrap() as id;
            let btn:  id = *STEP_NEXT_BTN_PTR.get().unwrap() as id;
            let _: () = msg_send![head, setStringValue: NSString::alloc(nil).init_str("✓  Step done!")];
            let _: () = msg_send![body, setStringValue: NSString::alloc(nil).init_str("Nice work! Wrapping up…")];
            let reason_lbl: id = *STEP_REASON_PTR.get().unwrap() as id;
            let _: () = msg_send![reason_lbl, setStringValue: NSString::alloc(nil).init_str("")];
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
        let _: () = msg_send![hint_win(), orderOut: nil as id];
        let _: () = msg_send![dot_win(), setIgnoresMouseEvents: NO];
        LOAD_TICK.store(0, Ordering::SeqCst);
        APP_STATE.store(3, Ordering::SeqCst);
        let _: () = msg_send![dot_win(), orderFrontRegardless];
        redraw_dot();
    }
    fire_next_step();
}

pub unsafe fn show_completion() {
    let _: () = msg_send![dot_win(),  orderOut: nil as id];
    let _: () = msg_send![hint_win(), orderOut: nil as id];

    let goal = GOAL_TEXT.get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_default();

    let head: id = *STEP_HEAD_PTR.get().unwrap() as id;
    let body: id = *STEP_BODY_PTR.get().unwrap() as id;
    let btn:  id = *STEP_NEXT_BTN_PTR.get().unwrap() as id;

    let _: () = msg_send![head, setStringValue: NSString::alloc(nil).init_str("🎉  All done!")];
    let final_ans = FINAL_ANSWER.get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_default();
    let body_text = if !final_ans.is_empty() {
        final_ans.clone()
    } else if goal.is_empty() {
        "You completed all the steps — great job!".to_owned()
    } else {
        format!("You completed: {}", goal)
    };
    let _: () = msg_send![body, setStringValue: NSString::alloc(nil).init_str(&body_text)];
    let reason_lbl: id = *STEP_REASON_PTR.get().unwrap() as id;
    let reason_text = if !final_ans.is_empty() {
        format!("Goal: {goal}")
    } else {
        String::new()
    };
    let _: () = msg_send![reason_lbl, setStringValue: NSString::alloc(nil).init_str(&reason_text)];
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
