# Pointer AI

**Pointer** is a macOS floating AI tour-guide that lives above every app on your screen. Press `⌘⇧P`, type a goal in plain English, and Pointer guides you step by step — showing exactly where to click, type, or scroll — using a live screenshot fed to a vision-language model.

No browser extension. No app integration. No Electron. Works on any macOS app, any website, any native window. Multi-monitor aware.

---

## Table of Contents

- [How It Works](#how-it-works)
- [Quick Start](#quick-start)
- [Usage](#usage)
- [Architecture](#architecture)
- [C API & ObjC Interface Layer](#c-api--objc-interface-layer)
- [State Machine](#state-machine)
- [AI Request Pipeline](#ai-request-pipeline)
- [Multi-Monitor Support](#multi-monitor-support)
- [API Reference](#api-reference)
- [Supported Models & Providers](#supported-models--providers)
- [Configuration](#configuration)
- [Project Structure](#project-structure)
- [Building for Release](#building-for-release)
- [Logging](#logging)
- [Troubleshooting](#troubleshooting)

---

## How It Works

```
⌘⇧P pressed  (CGEventTap intercepts globally)
      │
      ▼
detect_cursor_screen()                          ← which monitor is the cursor on?
      │  CGGetActiveDisplayList + NSScreen.deviceDescription
      │  stores: origin_x/y, width, height, display_idx
      ▼
Floating dot appears at cursor position  (borderless NSWindow, level 25)
      │
      ▼
User clicks dot → NSPanel prompt bar slides in
      │
      ▼
User types goal → presses Return
      │
      ┌─────────────────────────────────────────────────────────┐
      │  STEP LOOP  (repeats until is_final=true)               │
      │                                                         │
      │  screencapture -x -m -D {N} -t png                     │
      │    ↓  only the cursor's screen                          │
      │  sips --resampleWidth {logical_w}                       │
      │    ↓  Retina 2× → logical pixels (e.g. 2880→1440)      │
      │  base64-encode PNG                                      │
      │    ↓                                                    │
      │  ┌──────────────────────────────────────────────────┐   │
      │  │  REASONING CHECK                                 │   │
      │  │  is_reasoning_model(model)?                      │   │
      │  │    yes (qwen3, deepseek-r, o1, o3, r1…)          │   │
      │  │      max_tokens = 8192   ← CoT needs headroom    │   │
      │  │    no (llama-4, gemini, claude…)                  │   │
      │  │      max_tokens = 1024                           │   │
      │  └──────────────────────────────────────────────────┘   │
      │    ↓                                                    │
      │  POST to vision API  (OpenAI Chat Completions)          │
      │    Authorization: Bearer {key}  (whitespace-stripped)   │
      │    HTTP-Referer + X-Title  (OpenRouter only)            │
      │    system: SYSTEM_PROMPT  (coordinate + scroll rules)   │
      │    user:   goal + app + window + url                    │
      │            + screenshot image_url                       │
      │            + "Steps done so far:" history               │
      │            + stuck/max hint  (if applicable)            │
      │    ↓                                                    │
      │  ┌──────────────────────────────────────────────────┐   │
      │  │  PLANNING / HISTORY CONTEXT                      │   │
      │  │  Every request carries the full step history:    │   │
      │  │    "1. Click — opened File menu"                 │   │
      │  │    "2. Click — selected Save As"  …              │   │
      │  │  AI uses history to plan the next action         │   │
      │  │  without repeating completed steps               │   │
      │  │                                                  │   │
      │  │  STUCK LOOP DETECTION                            │   │
      │  │  last 3 actions identical?                       │   │
      │  │    → inject STUCK LOOP hint: try different       │   │
      │  │      element or answer directly                  │   │
      │  │  step_count >= 20?                               │   │
      │  │    → inject MAX STEPS hint: force is_final=true  │   │
      │  └──────────────────────────────────────────────────┘   │
      │    ↓                                                    │
      │  ┌──────────────────────────────────────────────────┐   │
      │  │  RESPONSE PARSING                                │   │
      │  │  HTTP status checked  (-w "\n%{http_code}")      │   │
      │  │  non-200 → surface API error message             │   │
      │  │  content=null + finish_reason=length             │   │
      │  │    → "model hit max_tokens" error                │   │
      │  │  is_uitars_model?  → Thought:/Action: parser     │   │
      │  │  else              → JSON parser                 │   │
      │  │  x/y out of bounds → retry with scroll hint      │   │
      │  │  rate_limit        → sleep, retry                │   │
      │  │  up to 3 attempts total                          │   │
      │  └──────────────────────────────────────────────────┘   │
      │    ↓                                                    │
      │  Coordinate translation (per-screen):                   │
      │    lx    = ai_x × (screen_w / img_w)                   │
      │    ly    = ai_y × (screen_h / img_h)                   │
      │    ns_x  = screen_origin_x + lx                        │
      │    ns_y  = screen_origin_y + screen_h − ly             │
      │    cg_y  = primary_screen_h − ns_y                     │
      │    ↓                                                    │
      │  Dot animates to (ns_x, ns_y)  (smoothstep, 16 ticks)  │
      │  Step card + hint pill appear near target               │
      │    ↓                                                    │
      │  User performs action → clicks "I did it →"            │
      │    ↓                                                    │
      │  history.push("Step N — {action}: {description}")      │
      │    ↓                                                    │
      │  is_final=false? ──────────────────────────────────┐   │
      └─────────────────────────────────────────────────────┘   │
                                                                │
      is_final=true ◀─────────────────────────────────────────┘
            │
            ▼
      Completion card  →  auto-dismiss ~5 s
```

---

## Quick Start

### 1. Clone and build

```bash
git clone https://github.com/yourname/pointer.git
cd pointer
cargo build --release
```

### 2. Configure your API key

Press `⌘⇧P`, click the dot, press Escape, then right-click → **⚙ Model Settings**.

Or set an environment variable before running:

```bash
export GROQ_API_KEY=gsk_...
./target/release/pointer
```

### 3. Grant Accessibility permission

**System Settings → Privacy & Security → Accessibility** → add `pointer`

Required for the global `⌘⇧P` hotkey (CGEventTap) and click auto-advance.

---

## Usage

| Action | How |
|---|---|
| Show / hide overlay | `⌘⇧P` from any app |
| Open prompt | Click the floating dot |
| Submit goal | Type goal, press `Return` |
| Dismiss | `Escape` at any time |
| Advance step | Click `"I did it →"` or click the target element itself |
| Change dot colour | `⚙` → Pointer Colour |
| Change model / provider | `⚙` → Model Settings |

---

## Architecture

Pointer is a pure Rust macOS app. **No Electron, no Swift, no Xcode.** The UI, event handling, and system calls are driven directly from Rust using `cocoa`, `objc`, and `dispatch` crates — thin wrappers over the macOS C APIs.

```
┌─────────────────────────────────────────────────────────────────────┐
│  main thread  (NSRunLoop / AppKit)                                  │
│                                                                     │
│  AppController (ObjC class registered at runtime via ClassDecl)     │
│    ├─ pulse:       50 ms NSTimer — animation, state transitions     │
│    ├─ providerChanged:, testConnection:, applySettings:, etc.       │
│                                                                     │
│  NSWindow stack (all borderless, level 25, collection-behaviour 1) │
│    ├─ dot_win    — 40×40 PointerView  (custom NSView, drawRect:)    │
│    ├─ in_panel   — 360×52 NSPanel  (prompt text field)              │
│    ├─ step_win   — 320×210 NSPanel (step card)                      │
│    ├─ hint_win   — 168×26  NSPanel (action hint pill)               │
│    ├─ color_win  — 256×150 NSPanel (colour picker)                  │
│    └─ model_win  — 360×290 NSPanel (provider / model settings)      │
│                                                                     │
│  Global atomics  (state.rs)                                         │
│    ├─ APP_STATE: AtomicU8  (0–5)                                     │
│    ├─ CURSOR_SCREEN_ORIGIN_X/Y, CURSOR_SCREEN_W/H  (AtomicU64)      │
│    ├─ CURSOR_DISPLAY_IDX  (AtomicU32)                               │
│    └─ SCREEN_H_LOGICAL  (AtomicU64, primary screen height)          │
└─────────────────────────┬───────────────────────────────────────────┘
                          │  dispatch::Queue::main().exec_async(...)
┌─────────────────────────▼───────────────────────────────────────────┐
│  background thread  (CGEventTap run loop)                           │
│    ├─ CGEventTapCreate — intercepts key-down/up + left-mouse-down   │
│    ├─ ⌘⇧P  → dispatch toggle() to main thread                      │
│    ├─ Return / Delete / printable chars → dispatch process_key()    │
│    └─ Left-click near target (state=4) → dispatch on_next_step()    │
└─────────────────────────────────────────────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────────────────┐
│  agent thread  (std::thread::spawn per step)                        │
│    ├─ screencapture -x -m -D {N} -t png /tmp/pointer_tour_ss.png   │
│    ├─ sips --resampleWidth {logical_w}  (Retina → logical pixels)   │
│    ├─ base64-encode PNG                                             │
│    ├─ curl POST  (OpenAI chat completions, vision)                  │
│    │    ├─ Authorization: Bearer {key}  (whitespace-stripped)       │
│    │    ├─ HTTP-Referer + X-Title  (OpenRouter only)                │
│    │    └─ -w "\n%{http_code}"  (HTTP status always checked)        │
│    └─ parse JSON → translate coords → dispatch to main thread       │
└─────────────────────────────────────────────────────────────────────┘
```

### Threading model

| Thread | Role |
|---|---|
| **Main** | All AppKit UI, NSRunLoop, timer callbacks, window management |
| **Event tap** | `CFRunLoopRun` in a background thread; dispatches events to main via GCD |
| **Agent** | One `std::thread::spawn` per AI step; result dispatched back via `dispatch::Queue::main().exec_async` |

All shared state uses either `AtomicU8/U32/U64/Bool/Usize` (lock-free timer reads) or `OnceLock<Mutex<T>>` (config, agent instance).

---

## C API & ObjC Interface Layer

Pointer calls macOS system APIs directly from Rust using `extern "C"` blocks and the `objc` crate's `msg_send!` macro. No bridging headers, no `.m` files.

### ObjC runtime (objc crate)

```rust
// Equivalent to: [NSColor colorWithRed:0.1 green:0.5 blue:1.0 alpha:1.0]
let color: id = msg_send![
    class!(NSColor),
    colorWithRed: 0.1_f64
    green:        0.5_f64
    blue:         1.0_f64
    alpha:        1.0_f64
];
```

`id` is an opaque `*mut Object` pointer. `class!(NSColor)` resolves via `objc_getClass`. `sel!` / `sel_impl!` must be in scope in every module using `msg_send!`:

```rust
use objc::{class, msg_send, sel, sel_impl};
```

### Registering a custom ObjC class at runtime

```rust
let mut decl = ClassDecl::new("AppController", class!(NSObject)).unwrap();

extern "C" fn pulse(_this: &Object, _sel: Sel, _sender: id) { /* ... */ }
decl.add_method(sel!(pulse:), pulse as extern "C" fn(&Object, Sel, id));

let cls  = decl.register();
let ctrl: id = msg_send![cls, alloc];
let ctrl: id = msg_send![ctrl, init];
```

Selectors on `AppController`:

| Selector | Trigger | Handler |
|---|---|---|
| `pulse:` | 50 ms NSTimer | Animation loop, state transitions |
| `enterPressed:` | Return key in prompt | `to_loading()` |
| `nextStepPressed:` | "I did it →" button | `on_next_step_pressed()` |
| `colorSelected:` | Colour swatch click | Updates `DOT_COLOR` atomic |
| `toggleSettings:` | Gear icon click | Shows context menu |
| `openColorPanel:` | Menu item | Shows colour picker window |
| `openModelSettings:` | Menu item | Populates and shows model panel |
| `providerChanged:` | `NSPopUpButton` action | Swaps visible provider box |
| `testConnection:` | Button | Spawns agent thread to probe API |
| `applySettings:` | Button / Return | Strips key whitespace, saves `~/.pointer_config.json` |

### CGEventTap — global keyboard and mouse interception

```rust
extern "C" {
    fn CGEventTapCreate(
        tap:              u32,   // kCGSessionEventTap = 0
        place:            u32,   // kCGHeadInsertEventTap = 0
        options:          u32,   // kCGEventTapOptionDefault = 0
        eventsOfInterest: u64,   // bitmask: key_down | key_up | left_mouse_down
        callback: unsafe extern "C" fn(
            *const c_void, u32, *const c_void, *const c_void
        ) -> *const c_void,
        userInfo: *const c_void,
    ) -> *mut c_void;
}
```

Returning `null` from the callback **consumes** the event. Returning the original pointer passes it through. Pointer consumes key events in state 2 (input) so typing goes to the prompt, not the active app.

### CGGetActiveDisplayList — display enumeration

Used by `detect_cursor_screen()` to map `NSScreen` objects to `screencapture -D N` indices:

```rust
extern "C" {
    fn CGGetActiveDisplayList(
        max_displays:   u32,
        active_displays: *mut u32,  // array of CGDirectDisplayID
        display_count:  *mut u32,
    ) -> i32;
}
```

`screencapture -D N` uses 1-based indexing into this same ordered list. Matching is done by comparing `CGDirectDisplayID` from `CGGetActiveDisplayList` with the `NSScreenNumber` key from `NSScreen.deviceDescription`.

### Accessibility API — reading the focused window title

```rust
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> *const c_void;
    fn AXUIElementCopyAttributeValue(
        element:   *const c_void,
        attribute: *const c_void, // CFStringRef, e.g. "AXFocusedWindow"
        value:     *mut *const c_void,
    ) -> i32; // 0 = kAXErrorSuccess
}
```

Frontmost app PID from AppKit:

```rust
let ws: id    = msg_send![class!(NSWorkspace), sharedWorkspace];
let front: id = msg_send![ws, frontmostApplication];
let pid: i32  = msg_send![front, processIdentifier];
```

### Window layering and always-on-top

```rust
let _: () = msg_send![window, setLevel: 25_i64];             // above all apps
let _: () = msg_send![window, setCollectionBehavior: 1u64];  // all Spaces + fullscreen
```

The dot window uses `setIgnoresMouseEvents: YES` during step display so clicks pass through to the target app.

---

## State Machine

```
  0 hidden
    │  ⌘⇧P
    ▼
  1 dot          Floating dot appears. detect_cursor_screen() runs. Blink animation active.
    │  click dot
    ▼
  2 input        Prompt bar visible. Key events captured.
    │  Return
    ▼
  3 loading      Dot shows spinner. Agent thread spawned.
    │  AI responds
    ▼
  4 step         Dot animates to target. Step card + hint pill shown.
    │  "I did it →" or click near target
    ├─ is_final=false ──▶ 3  (fire_next_step)
    └─ is_final=true  ──▶ 5 done
                              │ auto-dismiss ~5 s
  ◀── Escape (any state > 0) ─┘
```

| Value | Name | Description |
|---|---|---|
| `0` | hidden | All windows hidden |
| `1` | dot | Dot visible, cursor screen detected |
| `2` | input | Prompt bar open, keyboard captured |
| `3` | loading | Dot spins, agent thread running |
| `4` | step | Dot at target, step card visible |
| `5` | done | Completion card, auto-dismiss |

---

## AI Request Pipeline

Each step runs on a background thread.

### 1. Detect cursor screen

`detect_cursor_screen(mouse: NSPoint)` runs in `to_dot()` immediately when `⌘⇧P` is pressed:

```rust
// Find which NSScreen contains the cursor
for screen in NSScreen.screens {
    if screen.frame contains mouse {
        // Get CGDirectDisplayID from deviceDescription
        let cg_id: u32 = screen.deviceDescription["NSScreenNumber"];
        // Find 1-based index in CGGetActiveDisplayList order
        let disp_idx = active_displays.position(cg_id) + 1;
        // Store in atomics for the agent thread
        CURSOR_SCREEN_ORIGIN_X = screen.frame.origin.x;
        CURSOR_SCREEN_ORIGIN_Y = screen.frame.origin.y;
        CURSOR_SCREEN_W        = screen.frame.size.width;
        CURSOR_SCREEN_H        = screen.frame.size.height;
        CURSOR_DISPLAY_IDX     = disp_idx;
    }
}
// SCREEN_H_LOGICAL always = screens[0] height (primary, needed for CG ↔ AppKit conversion)
```

### 2. Capture the correct screen

```bash
screencapture -x -m -D {display_idx} -t png /tmp/pointer_tour_ss.png
# -D N  captures display N (1-based, same order as CGGetActiveDisplayList)
```

### 3. Downscale to logical resolution

Retina displays capture at 2× physical pixels (e.g. 2880×1800 for a 1440×900 logical screen). Vision models return coordinates in logical space regardless of image resolution. Sending 2× image causes a 2× positional error.

```bash
sips --resampleWidth {logical_w} /tmp/pointer_tour_ss.png
# In-place; height scales proportionally. AI coords now == logical points directly.
```

Log output: `screenshot 2880×1800 → 1440×900 sent to AI`

### 4. POST to vision API

```rust
let body = serde_json::json!({
    "model": model,
    "messages": [
        { "role": "system", "content": SYSTEM_PROMPT },
        { "role": "user", "content": [
            { "type": "text",      "text": user_context },
            { "type": "image_url", "image_url": { "url": "data:image/png;base64,..." } }
        ]}
    ],
    "temperature": 0,
    "max_tokens": 1024   // 8192 for reasoning models (qwen3, deepseek-r, o1, o3…)
});
```

Body written to `/tmp/pointer_body.json`, sent with `curl --data-binary @file` to avoid `ARG_MAX`.

**HTTP status code is always checked** (`-w "\n%{http_code}"`). Non-200 responses surface the API's own error message immediately rather than failing silently.

**OpenRouter** additionally requires:
```
HTTP-Referer: https://pointer.app
X-Title: Pointer
```

**API key sanitization:** All whitespace (including newlines from multi-paste) is stripped from API keys before sending and before saving to config.

**Provider + key logged at execution time:**
```
[INFO] provider: OpenRouter
[INFO] key:      sk-or-v1-0c715838...
[INFO] model:    qwen/qwen3.5-35b-a3b
[INFO] url:      https://openrouter.ai/api/v1/chat/completions
```

### 5. Coordinate translation (multi-screen)

```
AI returns (x, y) in image pixel space  (origin top-left, y down)
         ↓  scale to logical (no-op when image == logical res)
lx = ai_x × (screen_w / img_w)
ly = ai_y × (screen_h / img_h)
         ↓  translate to global AppKit space  (bottom-left origin)
ns_x = screen_origin_x + lx
ns_y = screen_origin_y + screen_h − ly
         ↓  convert to CoreGraphics space  (top-left of primary screen)
cg_y = primary_screen_h − ns_y
```

`SCREEN_H_LOGICAL` always stores the **primary** screen height (`screens[0]`) because the CG ↔ AppKit formula requires the primary screen's height regardless of which monitor the cursor is on.

### 6. Retry logic

Up to 3 attempts per step:

| Failure | Action |
|---|---|
| `rate_limit_exceeded` | Parse wait time from body, sleep, retry |
| Out-of-bounds coordinates | Inject scroll hint, retry |
| `max_tokens` (`content: null`, `finish_reason: "length"`) | Return clear error, retry |
| Non-200 HTTP status | Return `"HTTP {N} — {api message}"`, retry |
| JSON parse failure | Retry with same prompt |

**Stuck loop:** 3 identical consecutive actions → `STUCK LOOP DETECTED` hint injected.  
**Hard cap:** 20 steps → `MAX STEPS REACHED` forces `is_final=true`.

---

## Multi-Monitor Support

Pointer fully supports multi-monitor macOS setups. Here is the complete flow:

```
⌘⇧P pressed
│
├─ NSEvent.mouseLocation  →  cursor's global AppKit position
│
├─ detect_cursor_screen(mouse)
│     iterate NSScreen.screens
│     find frame containing mouse
│     read NSScreenNumber from deviceDescription
│     match to CGGetActiveDisplayList order → 1-based display index
│     store origin_x, origin_y, width, height, display_idx in atomics
│
├─ screencapture -D {display_idx}   →  captures only that screen
│
├─ sips --resampleWidth {logical_w} →  Retina → logical pixels
│
├─ AI returns (x, y) relative to captured screen image  (top-left, y-down)
│
└─ coordinate translation
      ns_x = origin_x + x                    ← global AppKit x
      ns_y = origin_y + screen_h − y         ← global AppKit y (bottom-left)
      cg_y = primary_h − ns_y                ← CG y for mouse events
```

**Why primary screen height?**  
CoreGraphics coordinate space has its origin at the top-left of the **primary** monitor. Converting AppKit → CG always requires subtracting from primary screen height, even when the action target is on a secondary monitor.

---

## API Reference

Pointer uses the **OpenAI-compatible Chat Completions API** with **vision (`image_url`)** support.

### Request

```http
POST https://api.groq.com/openai/v1/chat/completions
Authorization: Bearer gsk_...
Content-Type: application/json
```

```json
{
  "model": "meta-llama/llama-4-scout-17b-16e-instruct",
  "messages": [
    { "role": "system", "content": "You are a macOS GUI tour-guide..." },
    { "role": "user", "content": [
      { "type": "text", "text": "app:    Brave Browser\nwindow: GitHub\ngoal:   Star this repo\n\nScreenshot size: 1440 × 900 px  (origin TOP-LEFT, y DOWN)\nValid x range: 1–1439   Valid y range: 1–899\n\nSteps done so far:\nNone yet.\n\nIdentify the single next UI element and return its centre pixel." },
      { "type": "image_url", "image_url": { "url": "data:image/png;base64,..." } }
    ]}
  ],
  "temperature": 0,
  "max_tokens": 1024
}
```

### Response

Raw JSON, no markdown fences:

```json
{
  "x": 1312,
  "y": 148,
  "action": "Click",
  "description": "Click the Star button in the top-right of the repository",
  "reason": "The Star button is visible next to Watch and Fork",
  "is_final": false
}
```

### Response fields

| Field | Type | Required | Description |
|---|---|---|---|
| `x` | integer | No | Horizontal coordinate, image-space top-left origin |
| `y` | integer | No | Vertical coordinate, image-space top-left origin, y-down |
| `action` | string | Yes | `Click` `Double-click` `Right-click` `Type` `Scroll` `Hover` `Answer` |
| `description` | string | Yes | Shown in step card; final answer text when `is_final: true` |
| `reason` | string | No | Shown below description in step card |
| `is_final` | boolean | No | `true` = last step, triggers completion panel |

### max_tokens by model type

| Model family | `max_tokens` | Why |
|---|---|---|
| Standard vision models | `1024` | Enough for JSON output |
| Reasoning models (`qwen3`, `deepseek-r`, `o1`, `o3`, `o4`, `r1`, `r2`, `qwq`) | `8192` | Chain-of-thought consumes thousands of tokens before the JSON answer appears |

### UI-TARS models (self-hosted)

UI-TARS uses `Thought: / Action:` format with 0–999 normalised coordinates. Pointer auto-detects by model name (`ui-tars` / `uitars`) and switches format:

```
Thought: The Star button is visible in the top-right.
Action: click(point='<point>[[910,102]]</point>')
```

Coordinate conversion:
```
pixel_x = (norm_x / 999.0) × img_width
pixel_y = (norm_y / 999.0) × img_height
```

Supported actions: `click`, `double_click`, `right_click`, `hover`, `type`, `scroll`, `finished`.

---

## Supported Models & Providers

Pointer requires a **vision-capable** model accepting `image_url` content blocks. Test Connection runs a 1×1 PNG vision probe to verify this.

### Provider 0 — Groq

Fast inference, free tier. Default provider.

| Model | Notes |
|---|---|
| `meta-llama/llama-4-scout-17b-16e-instruct` | **Default** — best speed/accuracy |
| `meta-llama/llama-4-maverick-17b-128e-instruct` | Larger context |

Key at [console.groq.com](https://console.groq.com). Format: `gsk_...`

### Provider 1 — OpenRouter

Access to hundreds of models via one key. Required extra headers (`HTTP-Referer`, `X-Title`) are sent automatically.

| Model | Notes |
|---|---|
| `meta-llama/llama-4-maverick:free` | **Default** — free tier, vision |
| `google/gemini-2.0-flash-001` | Fast, accurate |
| `anthropic/claude-opus-4-5` | Highest accuracy |
| `qwen/qwen3.5-35b-a3b` | Reasoning model — slower but thorough |

> Reasoning models auto-detected by name get `max_tokens: 8192`. Non-reasoning models get `1024`.

Key at [openrouter.ai/keys](https://openrouter.ai/keys). Format: `sk-or-v1-...`

**Note:** Keys are stripped of all whitespace on save. Pasting a key multiple times into the field is safe — the duplicates are removed automatically.

### Provider 2 — Local / Self-hosted

Any OpenAI-compatible server with vision support.

| Server | Vision | Base URL |
|---|---|---|
| **vLLM** | Yes | `http://localhost:8000/v1` |
| **Ollama** | Yes (llava, llama3.2-vision) | `http://localhost:11434/v1` |
| **LM Studio** | Yes | `http://localhost:1234/v1` |

Use `EMPTY` as the API key for servers with no auth. The `/chat/completions` suffix is appended automatically.

**UI-TARS with vLLM:**
```bash
vllm serve bytedance-research/UI-TARS-7B-SFT --dtype bfloat16 --max-model-len 4096
```

---

## Configuration

### Config file — `~/.pointer_config.json`

Written on every Apply click. Loaded on startup.

```json
{
  "provider": 1,
  "groq_api_key": "gsk_...",
  "groq_model": "meta-llama/llama-4-scout-17b-16e-instruct",
  "groq_url": "https://api.groq.com/openai/v1/chat/completions",
  "openrouter_api_key": "sk-or-v1-...",
  "openrouter_model": "meta-llama/llama-4-maverick:free",
  "openrouter_url": "https://openrouter.ai/api/v1/chat/completions",
  "local_base_url": "http://localhost:8000/v1",
  "local_api_key": "EMPTY",
  "local_model": "bytedance-research/UI-TARS-7B-SFT"
}
```

| Field | Description |
|---|---|
| `provider` | `0` = Groq, `1` = OpenRouter, `2` = Local |
| `groq_api_key` | Groq API key (`gsk_...`) |
| `groq_model` | Vision-capable model on Groq |
| `groq_url` | Groq completions endpoint |
| `openrouter_api_key` | OpenRouter key (`sk-or-v1-...`) — whitespace stripped on save |
| `openrouter_model` | Model slug from [openrouter.ai/models](https://openrouter.ai/models) |
| `openrouter_url` | OpenRouter endpoint (defaults to `https://openrouter.ai/api/v1/chat/completions`) |
| `local_base_url` | Server base URL (`/chat/completions` appended automatically) |
| `local_api_key` | Auth token; `"EMPTY"` for no-auth servers |
| `local_model` | Model name as the server expects |

### Environment variables (new install defaults)

```bash
GROQ_API_KEY=gsk_...
GROQ_MODEL=meta-llama/llama-4-scout-17b-16e-instruct
GROQ_URL=https://api.groq.com/openai/v1/chat/completions
OPENROUTER_API_KEY=sk-or-v1-...
OPENROUTER_MODEL=meta-llama/llama-4-maverick:free
```

---

## Project Structure

```
src/
├── main.rs          AppController ObjC class, all selector handlers,
│                    NSWindow/NSPanel creation, startup bootstrap
├── config.rs        PointerConfig (serde JSON), load/save, effective_*() helpers, MsPtrs
├── state.rs         All global statics: AtomicU8/U32/U64/Bool/Usize,
│                    OnceLock<Mutex<T>>, cursor screen atomics, window accessors
├── tour.rs          State machine transitions, detect_cursor_screen(),
│                    context capture, show_step, on_next_step_pressed, show_completion
├── events.rs        CGEventTap setup + callback, key processing,
│                    AXIsProcessTrusted accessibility check
├── agent.rs         TourAgent, next_step(), capture_screenshot (with -D N),
│                    sips resize, curl POST with status check, JSON parsing,
│                    UI-TARS parser, is_reasoning_model(), is_uitars_model()
├── logger.rs        Daily-rotating log file (~/.pointer/logs/pointer_YYYY-MM-DD.log),
│                    log_info! / log_error! / log_ai! macros, UTC timestamps
└── ui/
    ├── mod.rs       PointerView (dot drawRect:), InputView, class registration
    ├── input.rs     Prompt bar panel (NSScrollView + NSTextField)
    ├── step.rs      Step guide panel (header, description, reason, button)
    ├── hint.rs      Action hint pill (frosted NSVisualEffectView)
    └── settings.rs  Colour picker (8 swatches),
                     Model Settings panel (NSPopUpButton + 3 provider boxes:
                     Groq / OpenRouter / Local — one visible at a time)
```

### Key types

```rust
// agent.rs
pub struct TourAgent {
    pub goal:            String,
    pub app:             String,    // frontmost app name
    pub window:          String,    // focused window title (AXUIElement)
    pub url:             Option<String>, // browser URL (AppleScript)
    pub screen_w:        f64,       // cursor screen logical width
    pub screen_h:        f64,       // cursor screen logical height
    pub screen_origin_x: f64,       // cursor screen AppKit origin x
    pub screen_origin_y: f64,       // cursor screen AppKit origin y
    pub display_num:     usize,     // 1-based screencapture -D index
    pub api_key:         String,
    pub api_url:         String,
    pub model:           String,
    history:             Vec<String>,  // "Step N — Action: description"
    step_num:            usize,
}

pub struct AgentStep {
    pub x:           f64,     // global AppKit x  (bottom-left origin)
    pub y:           f64,     // global AppKit y  (bottom-left origin)
    pub action:      String,
    pub description: String,
    pub reason:      String,
    pub step_num:    usize,
    pub is_final:    bool,
}

// config.rs
pub struct PointerConfig {
    pub provider:           u8,    // 0=Groq 1=OpenRouter 2=Local
    pub groq_api_key:       String,
    pub groq_model:         String,
    pub groq_url:           String,
    pub openrouter_api_key: String,
    pub openrouter_model:   String,
    pub openrouter_url:     String,
    pub local_base_url:     String,
    pub local_api_key:      String,
    pub local_model:        String,
}
// effective_api_key() / effective_model() / effective_api_url() resolve by provider index
```

### Atomic float storage

`AtomicU64` stores `f64` via bit-casting:

```rust
CURSOR_SCREEN_W.store(width.to_bits(),  Ordering::SeqCst);
let w = f64::from_bits(CURSOR_SCREEN_W.load(Ordering::SeqCst));
```

---

## Building for Release

```bash
cargo build --release
# Binary: ./target/release/pointer  (~6 MB stripped)
```

**Run at login:**  
System Settings → General → Login Items → add `pointer`

**Code-sign (optional):**
```bash
codesign --force --deep -s "Developer ID Application: Your Name" ./target/release/pointer
```

---

## Logging

Logs are written to `~/.pointer/logs/pointer_YYYY-MM-DD.log`. A new file is created each day.

```
══════════════════════════════════════════
Pointer started  2026-04-21 11:08:44 UTC
Log file: /Users/you/.pointer/logs/pointer_2026-04-21.log
══════════════════════════════════════════
[2026-04-21 11:08:44 UTC] [INFO ] provider: OpenRouter
[2026-04-21 11:08:44 UTC] [INFO ] key:      sk-or-v1-0c715838...
[2026-04-21 11:08:44 UTC] [INFO ] model:    qwen/qwen3.5-35b-a3b
[2026-04-21 11:08:44 UTC] [INFO ] url:      https://openrouter.ai/api/v1/chat/completions
[2026-04-21 11:08:44 UTC] [INFO ] app:    Canva
[2026-04-21 11:08:44 UTC] [INFO ] window: Canva — Design editor
[2026-04-21 11:08:44 UTC] [INFO ] query:  draw a circle
[2026-04-21 11:08:50 UTC] [INFO ] screenshot 2880×1800 → 1440×900 sent to AI
[2026-04-21 11:08:56 UTC] [AI   ] → {"x":428,"y":450,"action":"Click",...}
```

Log levels: `INFO`, `ERROR`, `AI` (raw model response). All logs also print to stderr.

---

## Troubleshooting

**`✗ Could not connect to server`**
- API URL field is empty or invalid — check Model Settings
- API key contains extra whitespace or was pasted multiple times — click Apply to re-save (whitespace is stripped automatically)
- Network / firewall issue

**`✗ Invalid API key`**
- HTTP 401 from the provider — verify the key in Model Settings
- OpenRouter keys start with `sk-or-v1-`; Groq keys start with `gsk_`

**`✗ HTTP 400 — input required: specific 'prompt'`**
- The selected model uses the `/completions` endpoint (text completion), not `/chat/completions`
- Switch to a chat/vision model on OpenRouter

**`✗ HTTP 400 — model does not support image input`**
- Text-only model selected — switch to a vision model
- On OpenRouter, embedding models (`gemini-embedding-*`, `text-embedding-*`) return this error

**`model hit max_tokens mid-reasoning`**
- A reasoning model ran out of tokens — now auto-allocated `8192` for detected reasoning models
- If still failing, try a smaller reasoning model or a non-reasoning vision model

**Dot clicks on wrong monitor**
- Fixed: Pointer captures the screen the cursor is on (`-D N`) and translates coordinates relative to that screen's origin

**Dot clicks slightly wrong position on correct monitor**
- Check `screenshot 2880×1800 → 1440×900` appears in logs — downscale must run for coordinates to be correct
- If image dimensions are unusual, verify `sips` is available: `which sips`

**Hotkey `⌘⇧P` does nothing**
- Accessibility not granted: System Settings → Privacy & Security → Accessibility → add `pointer`

**AI loops on the same action**
- Stuck-loop detection fires after 3 identical consecutive actions
- 20-step hard cap forces a final answer
- Try a model with stronger instruction-following (Llama-4 Scout, Gemini Flash, Claude)

---

## License

MIT
