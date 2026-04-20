use cocoa::base::{id, nil, NO, YES};
use cocoa::foundation::{NSPoint, NSRect, NSSize, NSString};
use objc::{class, msg_send, sel, sel_impl};
use std::sync::atomic::Ordering;

use crate::agent;
use crate::state::*;

// ── Colour-picker panel ───────────────────────────────────────────────────────

pub unsafe fn build_color_panel(ctrl: id) -> id {
    let panel: id = msg_send![class!(NSPanel), alloc];
    let panel: id = msg_send![panel,
        initWithContentRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(256.0, 150.0))
        styleMask: 3u64 backing: 2u64 defer: NO
    ];
    let _: () = msg_send![panel, setTitle: NSString::alloc(nil).init_str("Pointer Colour")];
    let _: () = msg_send![panel, setReleasedWhenClosed: NO];
    let _: () = msg_send![panel, setFloatingPanel: YES];
    let _: () = msg_send![panel, setHidesOnDeactivate: NO];
    let _: () = msg_send![panel, setCollectionBehavior: 1u64];

    let content: id = msg_send![panel, contentView];

    let colors: [(f64, f64, f64); 8] = [
        (0.10, 0.50, 1.00), (1.00, 0.22, 0.15), (0.12, 0.75, 0.30), (1.00, 0.55, 0.05),
        (0.65, 0.20, 0.90), (1.00, 0.25, 0.60), (0.00, 0.75, 0.75), (0.95, 0.80, 0.00),
    ];
    let sz    = 26.0_f64;
    let gap   = 12.0_f64;
    let cols  = 4_usize;
    let row_w = cols as f64 * sz + (cols - 1) as f64 * gap;
    let x0    = (256.0 - row_w) / 2.0;
    let row1_y = 80.0_f64;
    let row2_y = 44.0_f64;
    let cur = DOT_COLOR.load(Ordering::SeqCst) as usize;

    let mut btn_ptrs = [0usize; 8];
    for (i, &(r, g, b)) in colors.iter().enumerate() {
        let col_idx = i % cols;
        let row_y   = if i < cols { row1_y } else { row2_y };
        let sx = x0 + col_idx as f64 * (sz + gap);

        let btn: id = msg_send![class!(NSButton), alloc];
        let btn: id = msg_send![btn,
            initWithFrame: NSRect::new(NSPoint::new(sx, row_y), NSSize::new(sz, sz))
        ];
        let _: () = msg_send![btn, setBezelStyle: 0_i64];
        let _: () = msg_send![btn, setBordered: NO];
        let _: () = msg_send![btn, setWantsLayer: YES];
        let bl: id  = msg_send![btn, layer];
        let col: id = msg_send![class!(NSColor), colorWithRed:r green:g blue:b alpha:1.0_f64];
        let cg: id  = msg_send![col, CGColor];
        let _: () = msg_send![bl, setBackgroundColor: cg];
        let _: () = msg_send![bl, setCornerRadius: sz / 2.0];
        if i == cur {
            let ac: id    = msg_send![class!(NSColor), controlAccentColor];
            let ac_cg: id = msg_send![ac, CGColor];
            let _: () = msg_send![bl, setBorderColor: ac_cg];
            let _: () = msg_send![bl, setBorderWidth: 3.0_f64];
        }
        let _: () = msg_send![btn, setTitle: NSString::alloc(nil).init_str("")];
        let _: () = msg_send![btn, setTag: i as i64];
        let _: () = msg_send![btn, setAction: sel!(colorSelected:)];
        let _: () = msg_send![btn, setTarget: ctrl];
        let _: () = msg_send![content, addSubview: btn];
        btn_ptrs[i] = btn as usize;
    }
    COLOR_BTN_PTRS.set(btn_ptrs).ok();
    panel
}

// ── Model-settings panel ──────────────────────────────────────────────────────

pub unsafe fn build_model_settings_panel(ctrl: id) -> id {
    let panel: id = msg_send![class!(NSPanel), alloc];
    let panel: id = msg_send![panel,
        initWithContentRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(360.0, 284.0))
        styleMask: 3u64 backing: 2u64 defer: NO
    ];
    let _: () = msg_send![panel, setTitle: NSString::alloc(nil).init_str("Model Settings")];
    let _: () = msg_send![panel, setReleasedWhenClosed: NO];
    let _: () = msg_send![panel, setFloatingPanel: YES];
    let _: () = msg_send![panel, setHidesOnDeactivate: NO];
    let _: () = msg_send![panel, setCollectionBehavior: 1u64];

    let content: id = msg_send![panel, contentView];
    let pad = 20.0_f64;
    let fw  = 360.0 - pad * 2.0;

    let make_label = |y: f64, h: f64, text: &str, bold: bool| -> id {
        let f: id = msg_send![class!(NSTextField), alloc];
        let f: id = msg_send![f,
            initWithFrame: NSRect::new(NSPoint::new(pad, y), NSSize::new(fw, h))
        ];
        let _: () = msg_send![f, setEditable: NO];
        let _: () = msg_send![f, setBezeled: NO];
        let _: () = msg_send![f, setDrawsBackground: NO];
        let _: () = msg_send![f, setSelectable: NO];
        let font: id = if bold {
            msg_send![class!(NSFont), boldSystemFontOfSize: 12.0_f64]
        } else {
            msg_send![class!(NSFont), systemFontOfSize: 11.0_f64]
        };
        let _: () = msg_send![f, setFont: font];
        let c: id = msg_send![class!(NSColor), secondaryLabelColor];
        let _: () = msg_send![f, setTextColor: c];
        let _: () = msg_send![f, setStringValue: NSString::alloc(nil).init_str(text)];
        f
    };
    let make_field = |y: f64, ph: &str| -> id {
        let f: id = msg_send![class!(NSTextField), alloc];
        let f: id = msg_send![f,
            initWithFrame: NSRect::new(NSPoint::new(pad, y), NSSize::new(fw, 22.0))
        ];
        let _: () = msg_send![f, setEditable: YES];
        let _: () = msg_send![f, setBezeled: YES];
        let font: id = msg_send![class!(NSFont), systemFontOfSize: 13.0_f64];
        let _: () = msg_send![f, setFont: font];
        let cell: id = msg_send![f, cell];
        let _: () = msg_send![cell, setPlaceholderString: NSString::alloc(nil).init_str(ph)];
        f
    };

    // Provider segment
    let _: () = msg_send![content, addSubview: make_label(259.0, 13.0, "PROVIDER", false)];
    let seg: id = msg_send![class!(NSSegmentedControl), alloc];
    let seg: id = msg_send![seg,
        initWithFrame: NSRect::new(NSPoint::new(pad, 231.0), NSSize::new(fw, 24.0))
    ];
    let _: () = msg_send![seg, setSegmentCount: 2_i64];
    let _: () = msg_send![seg, setLabel: NSString::alloc(nil).init_str("Groq") forSegment: 0_i64];
    let _: () = msg_send![seg,
        setLabel: NSString::alloc(nil).init_str("Local / Self-hosted") forSegment: 1_i64];
    let _: () = msg_send![seg, setSelectedSegment: 0_i64];
    let _: () = msg_send![seg, setTrackingMode: 1_i64];
    let _: () = msg_send![seg, setAction: sel!(providerChanged:)];
    let _: () = msg_send![seg, setTarget: ctrl];
    let _: () = msg_send![content, addSubview: seg];

    // Top separator
    let sep_top: id = msg_send![class!(NSBox), alloc];
    let sep_top: id = msg_send![sep_top,
        initWithFrame: NSRect::new(NSPoint::new(pad, 223.0), NSSize::new(fw, 1.0))
    ];
    let _: () = msg_send![sep_top, setBoxType: 2_i64];
    let _: () = msg_send![content, addSubview: sep_top];

    // Groq fields
    let groq_box: id = msg_send![class!(NSView), alloc];
    let groq_box: id = msg_send![groq_box,
        initWithFrame: NSRect::new(NSPoint::new(0.0, 73.0), NSSize::new(360.0, 148.0))
    ];
    let _: () = msg_send![groq_box, addSubview: make_label(135.0, 11.0, "API KEY", false)];
    let groq_api = make_field(113.0, "gsk_…");
    let _: () = msg_send![groq_box, addSubview: groq_api];
    let _: () = msg_send![groq_box, addSubview: make_label(90.0, 11.0, "MODEL", false)];
    let groq_mdl = make_field(68.0, agent::GROQ_MODEL_DEFAULT);
    let _: () = msg_send![groq_box, addSubview: groq_mdl];
    let _: () = msg_send![groq_box, addSubview: make_label(45.0, 11.0, "API URL", false)];
    let groq_url = make_field(23.0, agent::GROQ_URL_DEFAULT);
    let _: () = msg_send![groq_box, addSubview: groq_url];
    let _: () = msg_send![content, addSubview: groq_box];

    // Local / self-hosted fields (hidden by default)
    let loc_box: id = msg_send![class!(NSView), alloc];
    let loc_box: id = msg_send![loc_box,
        initWithFrame: NSRect::new(NSPoint::new(0.0, 73.0), NSSize::new(360.0, 148.0))
    ];
    let _: () = msg_send![loc_box, setHidden: YES];
    let _: () = msg_send![loc_box, addSubview: make_label(135.0, 11.0, "BASE URL", false)];
    let loc_url = make_field(113.0, agent::UITARS_URL_EXAMPLE);
    let _: () = msg_send![loc_box, addSubview: loc_url];
    let _: () = msg_send![loc_box,
        addSubview: make_label(90.0, 11.0, "API KEY (use EMPTY if none)", false)];
    let loc_key = make_field(68.0, "EMPTY");
    let _: () = msg_send![loc_box, addSubview: loc_key];
    let _: () = msg_send![loc_box, addSubview: make_label(45.0, 11.0, "MODEL NAME", false)];
    let loc_mdl = make_field(23.0, agent::UITARS_MODEL_EXAMPLE);
    let _: () = msg_send![loc_box, addSubview: loc_mdl];
    let _: () = msg_send![content, addSubview: loc_box];

    // Bottom separator
    let sep_bot: id = msg_send![class!(NSBox), alloc];
    let sep_bot: id = msg_send![sep_bot,
        initWithFrame: NSRect::new(NSPoint::new(pad, 67.0), NSSize::new(fw, 1.0))
    ];
    let _: () = msg_send![sep_bot, setBoxType: 2_i64];
    let _: () = msg_send![content, addSubview: sep_bot];

    // Status label
    let status: id = msg_send![class!(NSTextField), alloc];
    let status: id = msg_send![status,
        initWithFrame: NSRect::new(NSPoint::new(pad, 48.0), NSSize::new(fw, 14.0))
    ];
    let _: () = msg_send![status, setEditable: NO];
    let _: () = msg_send![status, setBezeled: NO];
    let _: () = msg_send![status, setDrawsBackground: NO];
    let _: () = msg_send![status, setSelectable: NO];
    let sf: id = msg_send![class!(NSFont), systemFontOfSize: 11.0_f64];
    let _: () = msg_send![status, setFont: sf];
    let sc: id = msg_send![class!(NSColor), secondaryLabelColor];
    let _: () = msg_send![status, setTextColor: sc];
    let _: () = msg_send![status, setStringValue: NSString::alloc(nil).init_str("")];
    let _: () = msg_send![content, addSubview: status];

    // Test Connection button
    let test_btn: id = msg_send![class!(NSButton), alloc];
    let test_btn: id = msg_send![test_btn,
        initWithFrame: NSRect::new(NSPoint::new(pad, 12.0), NSSize::new(148.0, 28.0))
    ];
    let _: () = msg_send![test_btn, setBezelStyle: 1_i64];
    let _: () = msg_send![test_btn, setTitle: NSString::alloc(nil).init_str("Test Connection")];
    let _: () = msg_send![test_btn, setAction: sel!(testConnection:)];
    let _: () = msg_send![test_btn, setTarget: ctrl];
    let _: () = msg_send![content, addSubview: test_btn];

    // Apply button
    let apply_btn: id = msg_send![class!(NSButton), alloc];
    let apply_btn: id = msg_send![apply_btn,
        initWithFrame: NSRect::new(NSPoint::new(192.0, 12.0), NSSize::new(148.0, 28.0))
    ];
    let _: () = msg_send![apply_btn, setBezelStyle: 1_i64];
    let _: () = msg_send![apply_btn, setTitle: NSString::alloc(nil).init_str("Apply")];
    let _: () = msg_send![apply_btn, setKeyEquivalent: NSString::alloc(nil).init_str("\r")];
    let _: () = msg_send![apply_btn, setAction: sel!(applySettings:)];
    let _: () = msg_send![apply_btn, setTarget: ctrl];
    let _: () = msg_send![content, addSubview: apply_btn];

    use crate::config::MsPtrs;
    MS_PTRS.set(MsPtrs {
        provider_seg: seg      as usize,
        groq_api_fld: groq_api as usize,
        groq_mdl_fld: groq_mdl as usize,
        groq_url_fld: groq_url as usize,
        loc_url_fld:  loc_url  as usize,
        loc_key_fld:  loc_key  as usize,
        loc_mdl_fld:  loc_mdl  as usize,
        groq_box:     groq_box as usize,
        loc_box:      loc_box  as usize,
        status_lbl:   status   as usize,
    }).ok();
    panel
}
