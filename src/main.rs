#![allow(non_snake_case)]

mod agent;
mod config;
mod events;
pub mod logger;
mod state;
mod tour;
mod ui;

use cocoa::appkit::{
    NSApp, NSApplication, NSApplicationActivationPolicyAccessory,
    NSBackingStoreType, NSWindow, NSWindowStyleMask,
};
use cocoa::base::{id, nil, BOOL, NO, YES};
use cocoa::foundation::{NSAutoreleasePool, NSPoint, NSRect, NSSize, NSString};
use objc::declare::ClassDecl;
use objc::runtime::{Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::thread;

use config::{load_config, save_config};
use state::*;
use tour::{on_next_step_pressed, show_completion, to_hidden, to_loading};

fn main() {
    unsafe {
        let _pool = NSAutoreleasePool::new(nil);
        let app   = NSApp();
        app.setActivationPolicy_(NSApplicationActivationPolicyAccessory);
        let menu: id = msg_send![class!(NSMenu), new];
        let _: () = msg_send![app, setMainMenu: menu];

        logger::init();
        events::ensure_accessibility();

        POINTER_CONFIG.set(Mutex::new(load_config())).unwrap();
        INPUT_TEXT   .set(Mutex::new(String::new())).unwrap();
        LAST_CONTEXT .set(Mutex::new((String::new(), String::new()))).unwrap();
        TOUR_AGENT   .set(Mutex::new(None)).unwrap();
        GOAL_TEXT    .set(Mutex::new(String::new())).unwrap();
        FINAL_ANSWER .set(Mutex::new(String::new())).unwrap();

        let view_class = ui::register_pointer_view();

        // ── AppController ─────────────────────────────────────────────────────

        let mut ctrl_decl = ClassDecl::new("AppController", class!(NSObject))
            .expect("AppController already declared");

        extern "C" fn pulse(_: &Object, _: Sel, _: id) {
            // Keep OVERLAY_ACTIVE in sync with model-settings panel visibility
            // so the event tap lets Cmd+C/V etc. pass through while it's open.
            if MODEL_WIN_PTR.get().is_some() {
                unsafe {
                    let vis: BOOL = msg_send![model_win(), isVisible];
                    OVERLAY_ACTIVE.store(vis == YES, Ordering::SeqCst);
                }
            }
            let state = APP_STATE.load(Ordering::SeqCst);
            match state {
                1 | 4 => {
                    let tick = BLINK_TICK.fetch_add(1, Ordering::SeqCst);
                    let s = ((tick as f32) * 0.21).sin();
                    BLINK_ALPHA.store(
                        (0.22_f32 + 0.71_f32 * s * s).to_bits(), Ordering::SeqCst,
                    );
                    if state == 4 {
                        const ANIM_TICKS: u32 = 16;
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
                                let _: () = msg_send![dot_win(),
                                    setFrame: NSRect::new(
                                        NSPoint::new(cx - DOT_SZ / 2.0, cy - DOT_SZ / 2.0),
                                        NSSize::new(DOT_SZ, DOT_SZ))
                                    display: NO
                                ];
                            }
                        }
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

        extern "C" fn color_selected(_: &Object, _: Sel, sender: id) {
            unsafe {
                let tag: i64 = msg_send![sender, tag];
                DOT_COLOR.store(tag as u8, Ordering::SeqCst);
                if let Some(ptrs) = COLOR_BTN_PTRS.get() {
                    for (i, &ptr) in ptrs.iter().enumerate() {
                        let btn: id   = ptr as id;
                        let layer: id = msg_send![btn, layer];
                        if i == tag as usize {
                            let ac: id    = msg_send![class!(NSColor), controlAccentColor];
                            let ac_cg: id = msg_send![ac, CGColor];
                            let _: () = msg_send![layer, setBorderColor: ac_cg];
                            let _: () = msg_send![layer, setBorderWidth: 3.0_f64];
                        } else {
                            let _: () = msg_send![layer, setBorderWidth: 0.0_f64];
                        }
                    }
                }
                redraw_dot();
            }
        }

        extern "C" fn toggle_settings(this: &Object, _: Sel, sender: id) {
            unsafe {
                let ctrl: id = this as *const Object as id;
                let menu: id = msg_send![class!(NSMenu), alloc];
                let menu: id = msg_send![menu,
                    initWithTitle: NSString::alloc(nil).init_str("")
                ];
                let _: () = msg_send![menu, setAutoenablesItems: NO];
                for (title, sel) in &[
                    ("Pointer Colour", sel!(openColorPanel:)),
                    ("Model Settings", sel!(openModelSettings:)),
                ] {
                    let item: id = msg_send![class!(NSMenuItem), alloc];
                    let item: id = msg_send![item,
                        initWithTitle: NSString::alloc(nil).init_str(title)
                        action: *sel
                        keyEquivalent: NSString::alloc(nil).init_str("")
                    ];
                    let _: () = msg_send![item, setTarget: ctrl];
                    let _: () = msg_send![item, setEnabled: YES];
                    let _: () = msg_send![menu, addItem: item];
                }
                let loc = NSPoint::new(0.0, 0.0);
                let _: BOOL = msg_send![menu,
                    popUpMenuPositioningItem: nil as id atLocation: loc inView: sender];
            }
        }

        extern "C" fn open_color_panel(_: &Object, _: Sel, _: id) {
            unsafe {
                if COLOR_WIN_PTR.get().is_none() { return; }
                let w: id = color_win();
                let visible: BOOL = msg_send![w, isVisible];
                if visible == YES {
                    let _: () = msg_send![w, orderFront: nil as id];
                } else {
                    let _: () = msg_send![w, center];
                    let _: () = msg_send![w, makeKeyAndOrderFront: nil as id];
                }
            }
        }

        extern "C" fn open_model_settings(_: &Object, _: Sel, _: id) {
            unsafe {
                let Some(ptrs) = MS_PTRS.get() else { return };
                let cfg = POINTER_CONFIG.get().unwrap().lock().unwrap().clone();
                let _: () = msg_send![ptrs.provider_seg as id,
                    setSelectedSegment: cfg.provider as i64];
                let set_val = |ptr: usize, s: &str| {
                    let _: () = msg_send![ptr as id,
                        setStringValue: NSString::alloc(nil).init_str(s)];
                };
                set_val(ptrs.groq_api_fld, &cfg.groq_api_key);
                set_val(ptrs.groq_mdl_fld, &cfg.groq_model);
                set_val(ptrs.groq_url_fld, &cfg.groq_url);
                set_val(ptrs.loc_url_fld,  &cfg.local_base_url);
                set_val(ptrs.loc_key_fld,  &cfg.local_api_key);
                set_val(ptrs.loc_mdl_fld,  &cfg.local_model);
                set_val(ptrs.status_lbl,   "");
                let _: () = msg_send![ptrs.groq_box as id,
                    setHidden: if cfg.provider == 0 { NO } else { YES }];
                let _: () = msg_send![ptrs.loc_box as id,
                    setHidden: if cfg.provider == 0 { YES } else { NO }];
                let _: () = msg_send![model_win(), center];
                let _: () = msg_send![model_win(), makeKeyAndOrderFront: nil as id];
            }
        }

        extern "C" fn provider_changed(_: &Object, _: Sel, sender: id) {
            unsafe {
                let sel: i64 = msg_send![sender, selectedSegment];
                if let Some(ptrs) = MS_PTRS.get() {
                    let _: () = msg_send![ptrs.groq_box as id,
                        setHidden: if sel == 0 { NO } else { YES }];
                    let _: () = msg_send![ptrs.loc_box as id,
                        setHidden: if sel == 0 { YES } else { NO }];
                }
            }
        }

        extern "C" fn test_connection(_: &Object, _: Sel, _: id) {
            unsafe {
                let Some(ptrs) = MS_PTRS.get() else { return };
                let _: () = msg_send![ptrs.status_lbl as id,
                    setStringValue: NSString::alloc(nil).init_str("Testing…")];
                let seg: id   = ptrs.provider_seg as id;
                let prov: i64 = msg_send![seg, selectedSegment];
                let ns_str_val = |ptr: usize| -> String {
                    let v: id = msg_send![ptr as id, stringValue];
                    ns_str(v)
                };
                let (key, model, url) = if prov == 0 {
                    (
                        ns_str_val(ptrs.groq_api_fld),
                        ns_str_val(ptrs.groq_mdl_fld),
                        ns_str_val(ptrs.groq_url_fld),
                    )
                } else {
                    let base = ns_str_val(ptrs.loc_url_fld);
                    let full = format!("{}/chat/completions", base.trim_end_matches('/'));
                    (ns_str_val(ptrs.loc_key_fld), ns_str_val(ptrs.loc_mdl_fld), full)
                };
                let status_ptr = ptrs.status_lbl;
                agent::spawn(
                    move || agent::test_connection(&key, &model, &url),
                    move |result| {
                        let msg = match result {
                            Ok(())  => "✓ Connected".to_owned(),
                            Err(e)  => e,
                        };
                        let _: () = msg_send![status_ptr as id,
                            setStringValue: NSString::alloc(nil).init_str(&msg)];
                    },
                );
            }
        }

        extern "C" fn apply_settings(_: &Object, _: Sel, _: id) {
            unsafe {
                let Some(ptrs) = MS_PTRS.get() else { return };
                let seg: id   = ptrs.provider_seg as id;
                let prov: i64 = msg_send![seg, selectedSegment];
                let ns_str_val = |ptr: usize| -> String {
                    let v: id = msg_send![ptr as id, stringValue];
                    ns_str(v)
                };
                let mut cfg = POINTER_CONFIG.get().unwrap().lock().unwrap();
                cfg.provider       = prov as u8;
                cfg.groq_api_key   = ns_str_val(ptrs.groq_api_fld);
                cfg.groq_model     = ns_str_val(ptrs.groq_mdl_fld);
                cfg.groq_url       = ns_str_val(ptrs.groq_url_fld);
                cfg.local_base_url = ns_str_val(ptrs.loc_url_fld);
                cfg.local_api_key  = ns_str_val(ptrs.loc_key_fld);
                cfg.local_model    = ns_str_val(ptrs.loc_mdl_fld);
                save_config(&cfg);
                drop(cfg);
                let _: () = msg_send![model_win(), orderOut: nil as id];
            }
        }

        ctrl_decl.add_method(sel!(pulse:),             pulse             as extern "C" fn(&Object, Sel, id));
        ctrl_decl.add_method(sel!(enterPressed:),      enter_pressed     as extern "C" fn(&Object, Sel, id));
        ctrl_decl.add_method(sel!(nextStepPressed:),   next_step_pressed as extern "C" fn(&Object, Sel, id));
        ctrl_decl.add_method(sel!(colorSelected:),     color_selected    as extern "C" fn(&Object, Sel, id));
        ctrl_decl.add_method(sel!(toggleSettings:),    toggle_settings   as extern "C" fn(&Object, Sel, id));
        ctrl_decl.add_method(sel!(openColorPanel:),    open_color_panel  as extern "C" fn(&Object, Sel, id));
        ctrl_decl.add_method(sel!(openModelSettings:), open_model_settings as extern "C" fn(&Object, Sel, id));
        ctrl_decl.add_method(sel!(providerChanged:),   provider_changed  as extern "C" fn(&Object, Sel, id));
        ctrl_decl.add_method(sel!(testConnection:),    test_connection   as extern "C" fn(&Object, Sel, id));
        ctrl_decl.add_method(sel!(applySettings:),     apply_settings    as extern "C" fn(&Object, Sel, id));
        let ctrl_class = ctrl_decl.register();
        let ctrl: id = msg_send![ctrl_class, alloc];
        let ctrl: id = msg_send![ctrl, init];

        // ── Dot window ────────────────────────────────────────────────────────

        let dot_win: id = NSWindow::alloc(nil).initWithContentRect_styleMask_backing_defer_(
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(DOT_SZ, DOT_SZ)),
            NSWindowStyleMask::NSBorderlessWindowMask,
            NSBackingStoreType::NSBackingStoreBuffered,
            NO,
        );
        let _: () = msg_send![dot_win, setOpaque: NO];
        let dot_clear: id = msg_send![class!(NSColor), clearColor];
        let _: () = msg_send![dot_win, setBackgroundColor: dot_clear];
        let _: () = msg_send![dot_win, setLevel: 25_i64];
        let _: () = msg_send![dot_win, setHasShadow: NO];
        let _: () = msg_send![dot_win, setCollectionBehavior: 1u64];

        let dot_view: id = msg_send![view_class, alloc];
        let dot_view: id = msg_send![dot_view,
            initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(DOT_SZ, DOT_SZ))
        ];
        let _: () = msg_send![dot_win, setContentView: dot_view];
        DOT_WIN_PTR .set(dot_win  as usize).unwrap();
        DOT_VIEW_PTR.set(dot_view as usize).unwrap();

        // ── Panels ────────────────────────────────────────────────────────────

        let input_panel = ui::build_input_panel(ctrl);
        IN_PANEL_PTR.set(input_panel as usize).unwrap();

        let step_panel = ui::build_step_panel(ctrl);
        STEP_WIN_PTR.set(step_panel as usize).unwrap();

        let hint_panel = ui::build_hint_panel();
        HINT_WIN_PTR.set(hint_panel as usize).unwrap();

        let color_panel = ui::build_color_panel(ctrl);
        COLOR_WIN_PTR.set(color_panel as usize).unwrap();

        let model_panel = ui::build_model_settings_panel(ctrl);
        MODEL_WIN_PTR.set(model_panel as usize).unwrap();

        // ── 50 ms animation timer ─────────────────────────────────────────────

        let _: id = msg_send![class!(NSTimer),
            scheduledTimerWithTimeInterval: 0.05_f64
            target: ctrl selector: sel!(pulse:) userInfo: nil as id repeats: YES
        ];

        thread::spawn(events::run_event_tap);
        app.run();
    }
}
