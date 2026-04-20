use cocoa::base::{id, nil, NO, YES};
use cocoa::foundation::{NSPoint, NSRect, NSSize, NSString};
use objc::{class, msg_send, sel, sel_impl};

use crate::state::*;
use crate::ui::register_input_view;

// ── Input panel (prompt bar with blur background) ─────────────────────────────

pub unsafe fn build_input_panel(ctrl: id) -> id {
    let panel: id = msg_send![class!(NSPanel), alloc];
    let panel: id = msg_send![panel,
        initWithContentRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(IN_W, IN_H))
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

    // Frosted-glass background (NSVisualEffectMaterial.popover = 6)
    let ve: id = msg_send![class!(NSVisualEffectView), alloc];
    let ve: id = msg_send![ve,
        initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(IN_W, IN_H))
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

    let cy    = IN_H / 2.0;
    let white: id = msg_send![class!(NSColor), whiteColor];

    // Layout: [14] text(262) [8] gear(28) [6] submit(32) [10]
    let sub_w  = 32.0_f64;
    let sub_x  = IN_W - 10.0 - sub_w;          // 318
    let gear_w = 28.0_f64;
    let gear_x = sub_x - 6.0 - gear_w;         // 284
    let txt_x  = 14.0_f64;
    let txt_w  = gear_x - 8.0 - txt_x;         // 262

    // Display label in a horizontal NSScrollView (no scrollbars) for overflow
    let th = 22.0_f64;
    let ty = cy - th / 2.0;
    let df: id = msg_send![class!(NSTextField), alloc];
    let df: id = msg_send![df,
        initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(txt_w, th))
    ];
    let _: () = msg_send![df, setEditable: NO];
    let _: () = msg_send![df, setSelectable: NO];
    let _: () = msg_send![df, setBezeled: NO];
    let _: () = msg_send![df, setDrawsBackground: NO];
    let df_font: id = msg_send![class!(NSFont), systemFontOfSize: 16.0_f64];
    let _: () = msg_send![df, setFont: df_font];
    let _: () = msg_send![df, setTextColor: white];
    let _: () = msg_send![df, setStringValue: NSString::alloc(nil).init_str("")];
    let df_cell: id = msg_send![df, cell];
    let _: () = msg_send![df_cell, setScrollable: YES];
    let _: () = msg_send![df_cell, setWraps: NO];
    let _: () = msg_send![df_cell, setLineBreakMode: 0_i64];

    let scroll: id = msg_send![class!(NSScrollView), alloc];
    let scroll: id = msg_send![scroll,
        initWithFrame: NSRect::new(NSPoint::new(txt_x, ty), NSSize::new(txt_w, th))
    ];
    let _: () = msg_send![scroll, setHasHorizontalScroller: NO];
    let _: () = msg_send![scroll, setHasVerticalScroller: NO];
    let _: () = msg_send![scroll, setDrawsBackground: NO];
    let _: () = msg_send![scroll, setBorderType: 0_i64];
    let _: () = msg_send![scroll, setDocumentView: df];
    let clip: id = msg_send![scroll, contentView];
    let _: () = msg_send![clip, setDrawsBackground: NO];
    let _: () = msg_send![ve, addSubview: scroll];

    // Placeholder
    let ph: id = msg_send![class!(NSTextField), alloc];
    let ph: id = msg_send![ph,
        initWithFrame: NSRect::new(NSPoint::new(txt_x, ty), NSSize::new(txt_w, th))
    ];
    let _: () = msg_send![ph, setEditable: NO];
    let _: () = msg_send![ph, setBezeled: NO];
    let _: () = msg_send![ph, setDrawsBackground: NO];
    let _: () = msg_send![ph, setSelectable: NO];
    let ph_font: id = msg_send![class!(NSFont), systemFontOfSize: 16.0_f64];
    let _: () = msg_send![ph, setFont: ph_font];
    let phc: id = msg_send![class!(NSColor), colorWithRed:1.0 green:1.0 blue:1.0 alpha:0.28_f64];
    let _: () = msg_send![ph, setTextColor: phc];
    let _: () = msg_send![ph, setStringValue: NSString::alloc(nil).init_str("What do you want to do?")];
    let _: () = msg_send![ve, addSubview: ph];

    // InputView (transparent key-capture region)
    let iv_cls = register_input_view();
    let iv: id = msg_send![iv_cls, alloc];
    let iv: id = msg_send![iv,
        initWithFrame: NSRect::new(NSPoint::new(txt_x, 0.0), NSSize::new(txt_w, IN_H))
    ];
    let _: () = msg_send![ve, addSubview: iv];

    // ⚙ Gear button
    let gear_btn: id = msg_send![class!(NSButton), alloc];
    let gear_btn: id = msg_send![gear_btn,
        initWithFrame: NSRect::new(
            NSPoint::new(gear_x, cy - gear_w / 2.0), NSSize::new(gear_w, gear_w))
    ];
    let _: () = msg_send![gear_btn, setBezelStyle: 0_i64];
    let _: () = msg_send![gear_btn, setBordered: NO];
    let _: () = msg_send![gear_btn, setTitle: NSString::alloc(nil).init_str("⚙")];
    let gear_font: id = msg_send![class!(NSFont), systemFontOfSize: 17.0_f64];
    let _: () = msg_send![gear_btn, setFont: gear_font];
    let gear_c: id = msg_send![class!(NSColor), colorWithRed:1.0 green:1.0 blue:1.0 alpha:0.50_f64];
    let _: () = msg_send![gear_btn, setContentTintColor: gear_c];
    let _: () = msg_send![gear_btn, setAction: sel!(toggleSettings:)];
    let _: () = msg_send![gear_btn, setTarget: ctrl];
    let _: () = msg_send![ve, addSubview: gear_btn];

    // ↩ Submit button
    let btn: id = msg_send![class!(NSButton), alloc];
    let btn: id = msg_send![btn,
        initWithFrame: NSRect::new(
            NSPoint::new(sub_x, cy - sub_w / 2.0), NSSize::new(sub_w, sub_w))
    ];
    let _: () = msg_send![btn, setBezelStyle: 0_i64];
    let _: () = msg_send![btn, setBordered: NO];
    let _: () = msg_send![btn, setWantsLayer: YES];
    let btnl: id = msg_send![btn, layer];
    let blue: id = msg_send![class!(NSColor), colorWithRed:0.07 green:0.47 blue:1.0 alpha:1.0_f64];
    let cg_blue: id = msg_send![blue, CGColor];
    let _: () = msg_send![btnl, setBackgroundColor: cg_blue];
    let _: () = msg_send![btnl, setCornerRadius: sub_w / 2.0];
    let _: () = msg_send![btn, setTitle: NSString::alloc(nil).init_str("\u{21A9}")];
    let btn_font: id = msg_send![class!(NSFont), systemFontOfSize: 15.0_f64];
    let _: () = msg_send![btn, setFont: btn_font];
    let _: () = msg_send![btn, setContentTintColor: white];
    let _: () = msg_send![btn, setAction: sel!(enterPressed:)];
    let _: () = msg_send![btn, setTarget: ctrl];
    let _: () = msg_send![ve, addSubview: btn];

    DISPLAY_FIELD_PTR .set(df     as usize).unwrap();
    DISPLAY_SCROLL_PTR.set(scroll as usize).unwrap();
    PLACEHOLDER_PTR   .set(ph     as usize).unwrap();
    INPUT_VIEW_PTR    .set(iv     as usize).unwrap();
    panel
}
