use cocoa::base::{id, nil, NO, YES};
use cocoa::foundation::{NSPoint, NSRect, NSSize, NSString};
use objc::{class, msg_send, sel, sel_impl};

use crate::state::*;

// ── Step panel ────────────────────────────────────────────────────────────────
//
//  210 ┌──────────────────────────────────────┐
//      │  Step N · ACTION          (11pt dim) │  y=188, h=18
//  186 │ ─────────────────────────────────── │  sep y=186
//      │  description (14pt white, 3 lines)   │  y=130, h=52
//      │  reason      (11.5pt dim, 3 lines)   │  y=78,  h=48
//   74 │ ─────────────────────────────────── │  sep y=74
//      │  [ I did it →  ]                    │  y=14,  h=40
//  ────┘

pub unsafe fn build_step_panel(ctrl: id) -> id {
    let panel: id = msg_send![class!(NSPanel), alloc];
    let panel: id = msg_send![panel,
        initWithContentRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(STEP_W, STEP_H))
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
    let _: () = msg_send![panel, setHidesOnDeactivate: NO];

    let ve: id = msg_send![class!(NSVisualEffectView), alloc];
    let ve: id = msg_send![ve,
        initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(STEP_W, STEP_H))
    ];
    let _: () = msg_send![ve, setMaterial: 6_i64];
    let _: () = msg_send![ve, setBlendingMode: 0_i64];
    let _: () = msg_send![ve, setState: 1_i64];
    let _: () = msg_send![ve, setWantsLayer: YES];
    let vel: id = msg_send![ve, layer];
    let _: () = msg_send![vel, setCornerRadius: 14.0_f64];
    let _: () = msg_send![vel, setMasksToBounds: YES];
    let _: () = msg_send![panel, setContentView: ve];
    force_dark(ve);

    let pad  = 16.0_f64;
    let cw   = STEP_W - pad * 2.0;
    let white: id = msg_send![class!(NSColor), whiteColor];
    let dim:   id = msg_send![class!(NSColor), colorWithRed:1.0 green:1.0 blue:1.0 alpha:0.50_f64];

    let make_lbl = |frame: NSRect, font: id, color: id, text: &str| -> id {
        let f: id = msg_send![class!(NSTextField), alloc];
        let f: id = msg_send![f, initWithFrame: frame];
        let _: () = msg_send![f, setEditable: NO];
        let _: () = msg_send![f, setBezeled: NO];
        let _: () = msg_send![f, setDrawsBackground: NO];
        let _: () = msg_send![f, setSelectable: NO];
        let _: () = msg_send![f, setFont: font];
        let _: () = msg_send![f, setTextColor: color];
        let _: () = msg_send![f, setStringValue: NSString::alloc(nil).init_str(text)];
        f
    };

    let make_sep = |y: f64| -> id {
        let s: id = msg_send![class!(NSView), alloc];
        let s: id = msg_send![s,
            initWithFrame: NSRect::new(NSPoint::new(pad, y), NSSize::new(cw, 1.0))
        ];
        let _: () = msg_send![s, setWantsLayer: YES];
        let sl: id = msg_send![s, layer];
        let c: id  = msg_send![class!(NSColor), colorWithRed:1.0 green:1.0 blue:1.0 alpha:0.12_f64];
        let cg: id = msg_send![c, CGColor];
        let _: () = msg_send![sl, setBackgroundColor: cg];
        s
    };

    // Header line
    let head_font: id = msg_send![class!(NSFont), systemFontOfSize: 11.0_f64];
    let head = make_lbl(
        NSRect::new(NSPoint::new(pad, 188.0), NSSize::new(cw, 18.0)),
        head_font, dim, "",
    );
    let _: () = msg_send![ve, addSubview: head];
    let _: () = msg_send![ve, addSubview: make_sep(186.0)];

    // Description
    let body_font: id = msg_send![class!(NSFont), systemFontOfSize: 14.0_f64];
    let body = make_lbl(
        NSRect::new(NSPoint::new(pad, 130.0), NSSize::new(cw, 52.0)),
        body_font, white, "",
    );
    let _: () = msg_send![body, setLineBreakMode: 6_i64];
    let _: () = msg_send![body, setMaximumNumberOfLines: 3_i64];
    let body_cell: id = msg_send![body, cell];
    let _: () = msg_send![body_cell, setWraps: YES];
    let _: () = msg_send![body_cell, setScrollable: NO];
    let _: () = msg_send![ve, addSubview: body];

    // Reason / why
    let reason_font:  id = msg_send![class!(NSFont), systemFontOfSize: 11.5_f64];
    let reason_color: id = msg_send![class!(NSColor),
        colorWithRed:1.0 green:1.0 blue:1.0 alpha:0.55_f64];
    let reason = make_lbl(
        NSRect::new(NSPoint::new(pad, 78.0), NSSize::new(cw, 48.0)),
        reason_font, reason_color, "",
    );
    let _: () = msg_send![reason, setLineBreakMode: 6_i64];
    let _: () = msg_send![reason, setMaximumNumberOfLines: 3_i64];
    let reason_cell: id = msg_send![reason, cell];
    let _: () = msg_send![reason_cell, setWraps: YES];
    let _: () = msg_send![reason_cell, setScrollable: NO];
    let _: () = msg_send![ve, addSubview: reason];
    let _: () = msg_send![ve, addSubview: make_sep(74.0)];

    // "I did it →" button
    let next_btn: id = msg_send![class!(NSButton), alloc];
    let next_btn: id = msg_send![next_btn,
        initWithFrame: NSRect::new(NSPoint::new(pad, 14.0), NSSize::new(cw, 40.0))
    ];
    let _: () = msg_send![next_btn, setBezelStyle: 0_i64];
    let _: () = msg_send![next_btn, setBordered: NO];
    let _: () = msg_send![next_btn, setWantsLayer: YES];
    let btnl: id = msg_send![next_btn, layer];
    let blue: id = msg_send![class!(NSColor), colorWithRed:0.07 green:0.47 blue:1.0 alpha:1.0_f64];
    let cg_blue: id = msg_send![blue, CGColor];
    let _: () = msg_send![btnl, setBackgroundColor: cg_blue];
    let _: () = msg_send![btnl, setCornerRadius: 10.0_f64];
    let _: () = msg_send![next_btn, setTitle: NSString::alloc(nil).init_str("I did it  \u{2192}")];
    let btnf: id = msg_send![class!(NSFont), boldSystemFontOfSize: 14.0_f64];
    let _: () = msg_send![next_btn, setFont: btnf];
    let _: () = msg_send![next_btn, setContentTintColor: white];
    let _: () = msg_send![next_btn, setAction: sel!(nextStepPressed:)];
    let _: () = msg_send![next_btn, setTarget: ctrl];
    let _: () = msg_send![ve, addSubview: next_btn];

    STEP_HEAD_PTR    .set(head     as usize).unwrap();
    STEP_BODY_PTR    .set(body     as usize).unwrap();
    STEP_REASON_PTR  .set(reason   as usize).unwrap();
    STEP_NEXT_BTN_PTR.set(next_btn as usize).unwrap();
    panel
}
