use cocoa::base::{id, YES, NO};
use cocoa::foundation::{NSPoint, NSRect, NSSize};
use objc::{class, msg_send, sel, sel_impl};

use crate::state::*;

// ── Action-hint pill shown near the dot ───────────────────────────────────────

pub unsafe fn build_hint_panel() -> id {
    let panel: id = msg_send![class!(NSPanel), alloc];
    let panel: id = msg_send![panel,
        initWithContentRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(HINT_W, HINT_H))
        styleMask: 0u64 backing: 2u64 defer: NO
    ];
    let _: () = msg_send![panel, setOpaque: NO];
    let clear: id = msg_send![class!(NSColor), clearColor];
    let _: () = msg_send![panel, setBackgroundColor: clear];
    let _: () = msg_send![panel, setLevel: 25_i64];
    let _: () = msg_send![panel, setHasShadow: YES];
    let _: () = msg_send![panel, setIgnoresMouseEvents: YES];
    let _: () = msg_send![panel, setCollectionBehavior: 1u64];
    let _: () = msg_send![panel, setHidesOnDeactivate: NO];

    // Frosted pill (NSVisualEffectMaterialToolTip = 17)
    let ve: id = msg_send![class!(NSVisualEffectView), alloc];
    let ve: id = msg_send![ve,
        initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(HINT_W, HINT_H))
    ];
    let _: () = msg_send![ve, setMaterial: 17_i64];
    let _: () = msg_send![ve, setBlendingMode: 0_i64];
    let _: () = msg_send![ve, setState: 1_i64];
    let _: () = msg_send![ve, setWantsLayer: YES];
    let vel: id = msg_send![ve, layer];
    let _: () = msg_send![vel, setCornerRadius: HINT_H / 2.0];
    let _: () = msg_send![vel, setMasksToBounds: YES];
    let _: () = msg_send![panel, setContentView: ve];
    force_dark(ve);

    let lbl: id = msg_send![class!(NSTextField), alloc];
    let lbl: id = msg_send![lbl,
        initWithFrame: NSRect::new(
            NSPoint::new(4.0, 1.0), NSSize::new(HINT_W - 8.0, HINT_H - 2.0))
    ];
    let _: () = msg_send![lbl, setEditable: NO];
    let _: () = msg_send![lbl, setBezeled: NO];
    let _: () = msg_send![lbl, setDrawsBackground: NO];
    let _: () = msg_send![lbl, setSelectable: NO];
    let _: () = msg_send![lbl, setAlignment: 2_i64];
    let lf: id = msg_send![class!(NSFont), boldSystemFontOfSize: 11.0_f64];
    let _: () = msg_send![lbl, setFont: lf];
    let wc: id = msg_send![class!(NSColor), whiteColor];
    let _: () = msg_send![lbl, setTextColor: wc];
    let _: () = msg_send![ve, addSubview: lbl];

    HINT_LBL_PTR.set(lbl as usize).unwrap();
    panel
}
