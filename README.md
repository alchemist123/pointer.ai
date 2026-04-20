# Pointer AI

**Pointer** is a macOS floating AI tour-guide that lives above every app on your screen. Press `⌘⇧P`, type a goal in plain English, and Pointer guides you step by step — showing exactly where to click, type, or scroll — using a live screenshot fed to a vision-language model.

No browser extension. No app integration. Works on any macOS app, any website, any native window.

---

## Table of Contents

- [How It Works](#how-it-works)
- [Quick Start](#quick-start)
- [Usage](#usage)
- [Architecture](#architecture)
- [C API & ObjC Interface Layer](#c-api--objc-interface-layer)
- [State Machine](#state-machine)
- [AI Request Pipeline](#ai-request-pipeline)
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
Floating dot appears at cursor position  (borderless NSWindow, level 25)
      │
      ▼
User clicks dot → NSPanel prompt bar slides in
      │
      ▼
User types goal → presses Return
      │
      ▼
screencapture -m  →  sips --resampleWidth (downscale to logical res)
      │
      ▼
base64-encode PNG  →  POST to vision API  (OpenAI Chat Completions)
      │
      ▼
AI returns JSON: { x, y, action, description, reason, is_final }
      │
      ▼
Dot animates to (x, y)  (smoothstep over 16 timer ticks @ 50 ms)
Step card + hint pill appear near target element
      │
      ▼
User performs action → clicks "I did it →"  (or clicks the target directly)
      │
      ▼
Loop until is_final=true  →  Completion card  →  auto-dismiss in ~5 s
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

Open the binary, press `⌘⇧P`, click the dot, press Escape, then right-click the dot `⚙` → **Model Settings**.  
Or set an environment variable before running:

```bash
export GROQ_API_KEY=gsk_...
./target/release/pointer
```

### 3. Grant Accessibility permission

On first launch macOS prompts automatically. Or go to:

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

Pointer is a pure Rust macOS app. It has **no Electron, no Swift, no Xcode**. The UI, event handling, and system calls are all driven directly from Rust using the `cocoa`, `objc`, and `dispatch` crates, which are thin wrappers over the macOS C APIs.

```
┌─────────────────────────────────────────────────────────────────────┐
│  main thread  (NSRunLoop / AppKit)                                  │
│                                                                     │
│  AppController (ObjC class registered at runtime via ClassDecl)     │
│    ├─ pulse:       50 ms NSTimer — animation, state transitions     │
│    ├─ colorSelected:, providerChanged:, testConnection:, etc.       │
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
│    └─ APP_STATE: AtomicU8  (0–5), animation ticks, window ptrs      │
└─────────────────────────┬───────────────────────────────────────────┘
                          │  dispatch::Queue::main().exec_async(...)
                          │
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
│    ├─ screencapture -x -m -t png /tmp/pointer_tour_ss.png           │
│    ├─ sips --resampleWidth {logical_w}   (Retina → logical pixels)  │
│    ├─ base64-encode PNG                                             │
│    ├─ curl POST to vision API  (OpenAI-compatible)                  │
│    └─ parse JSON → dispatch result to main thread                   │
└─────────────────────────────────────────────────────────────────────┘
```

### Threading model

| Thread | Role |
|---|---|
| **Main** | All AppKit UI, NSRunLoop, timer callbacks, window management |
| **Event tap** | `CFRunLoopRun` in a background thread; dispatches key/click events to main via GCD |
| **Agent** | One `std::thread::spawn` per AI step; result dispatched back to main via `dispatch::Queue::main().exec_async` |

All shared state between threads uses either `AtomicU8/U32/U64/Bool/Usize` (lock-free reads in the timer) or `OnceLock<Mutex<T>>` (mutable but infrequently accessed data like config and the agent instance).

---

## C API & ObjC Interface Layer

Pointer calls macOS system APIs directly from Rust using `extern "C"` blocks and the `objc` crate's `msg_send!` macro. No bridging headers, no `.m` files.

### ObjC runtime (objc crate)

The `msg_send!` macro compiles to a direct call to `objc_msgSend`, which is the runtime dispatch function used by all Objective-C code:

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

`id` is an opaque `*mut Object` pointer — the same type used for all ObjC objects. `class!(NSColor)` resolves the class pointer at runtime via `objc_getClass`.

`sel!` / `sel_impl!` must be in scope whenever `msg_send!` is used because the macro internally generates `sel!(methodName:)` calls to resolve selectors:

```rust
use objc::{class, msg_send, sel, sel_impl};
```

### Registering a custom ObjC class at runtime

`AppController` and the two custom views (`PointerView`, `InputView`) are registered at runtime using `ClassDecl`, without any `.h` / `.m` files:

```rust
// Register a new ObjC class that inherits from NSObject
let mut decl = ClassDecl::new("AppController", class!(NSObject)).unwrap();

// Add a method — the Rust function signature must match what ObjC expects
extern "C" fn pulse(_this: &Object, _sel: Sel, _sender: id) { /* ... */ }
decl.add_method(
    sel!(pulse:),
    pulse as extern "C" fn(&Object, Sel, id),
);

let cls = decl.register(); // registers with the ObjC runtime
let ctrl: id = msg_send![cls, alloc];
let ctrl: id = msg_send![ctrl, init];
```

Selectors wired up on `AppController`:

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
| `applySettings:` | Button / Return | Saves config to `~/.pointer_config.json` |

### Custom NSView subclasses

`PointerView` (the animated dot) and `InputView` (invisible key-capture view in the prompt bar) override `drawRect:` and key event methods:

```rust
// drawRect: implementation registered on the class
extern "C" fn draw_rect(this: &Object, _: Sel, _dirty: NSRect) {
    unsafe {
        let ctx: id = msg_send![class!(NSGraphicsContext), currentContext];
        let cg: *mut c_void = msg_send![ctx, CGContext];
        // Call CoreGraphics C functions directly
        CGContextSetRGBFillColor(cg, r, g, b, alpha);
        CGContextFillEllipseInRect(cg, rect);
    }
}
decl.add_method(sel!(drawRect:), draw_rect as extern "C" fn(&Object, Sel, NSRect));
```

### CGEventTap — global keyboard and mouse interception

`CGEventTapCreate` registers a callback that fires for every key and mouse event system-wide, regardless of which app is focused:

```rust
extern "C" {
    fn CGEventTapCreate(
        tap:              u32,   // kCGSessionEventTap = 0
        place:            u32,   // kCGHeadInsertEventTap = 0
        options:          u32,   // kCGEventTapOptionDefault = 0
        eventsOfInterest: u64,   // bitmask: MASK_KEY_DOWN | MASK_KEY_UP | MASK_LEFT_MOUSE_DOWN
        callback: unsafe extern "C" fn(
            *const c_void, u32, *const c_void, *const c_void
        ) -> *const c_void,
        userInfo: *const c_void,
    ) -> *mut c_void;            // returns a CFMachPortRef
}
```

Inside the callback, key codes are read with `CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode)` and modifier flags with `CGEventGetFlags(event)`.

Returning `std::ptr::null()` **consumes** the event (it is not passed to the focused app). Returning the original `event` pointer passes it through. Pointer consumes key events when `APP_STATE == 2` (input mode) so typing goes to the prompt, not the active app.

The tap is added to a `CFRunLoop` on a dedicated background thread:

```rust
let src = CFMachPortCreateRunLoopSource(null, tap, 0);
CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopDefaultMode);
CFRunLoopRun(); // blocks this thread forever
```

If the tap is disabled by the OS (timeout or permission revoked), the callback receives `kCGEventTapDisabledByTimeout` and re-enables the tap with `CGEventTapEnable(tap, true)`.

### Accessibility API — reading the focused window title

Pointer reads the title of the frontmost window using `AXUIElement`, the macOS Accessibility API (also exposed as a C API):

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

The frontmost app's PID comes from AppKit:

```rust
let ws: id    = msg_send![class!(NSWorkspace), sharedWorkspace];
let front: id = msg_send![ws, frontmostApplication];
let pid: i32  = msg_send![front, processIdentifier];
```

The window title is then passed to the AI with every request so it knows what the user is looking at.

### Window layering and always-on-top

All Pointer windows use `NSWindow level 25` (above normal apps, below the menu bar) and `NSWindowCollectionBehavior 1` (appears on all Spaces / full-screen apps):

```rust
let _: () = msg_send![window, setLevel: 25_i64];
let _: () = msg_send![window, setCollectionBehavior: 1u64];
```

The dot window also uses `setIgnoresMouseEvents: YES` during step display so the user can click the target element underneath it. It's switched back to `NO` when the user needs to interact with the dot.

### NSVisualEffectView — frosted glass panels

All panels use `NSVisualEffectView` with `NSVisualEffectBlendingModeBehindWindow` and forced `NSAppearanceNameDarkAqua` for the dark frosted-glass look regardless of system appearance setting.

---

## State Machine

The app is driven by a single `AtomicU8` (`APP_STATE`) with 6 states. All transitions happen on the main thread.

```
  0 hidden
    │  ⌘⇧P
    ▼
  1 dot          Floating dot appears at cursor. Blink animation active.
    │  click dot
    ▼
  2 input        Prompt bar visible. Key events captured by CGEventTap.
    │  Return
    ▼
  3 loading      Dot shows spinner animation. Agent thread running.
    │  AI responds
    ▼
  4 step         Dot flies to target. Step card + hint pill shown.
    │  "I did it →" or click near target
    ├─ is_final=false ──▶ 3 loading  (fire_next_step)
    └─ is_final=true  ──▶ 5 done
                              │
  ◀── Escape ─────────────────┤  (from any state > 0)
                              │
                              ▼
                          0 hidden  (auto-dismiss after ~5 s in state 5)
```

### State values

| Value | Name | Description |
|---|---|---|
| `0` | hidden | All windows hidden, no timers active |
| `1` | dot | Dot visible, pulse blink animation |
| `2` | input | Prompt bar open, keyboard captured |
| `3` | loading | Dot spins, agent thread running |
| `4` | step | Dot at target, step card visible |
| `5` | done | Completion card, auto-dismiss after ~5 s |

---

## AI Request Pipeline

Each step follows this pipeline on a background thread:

### 1. Capture screenshot

```bash
screencapture -x -m -t png /tmp/pointer_tour_ss.png
# -x = no sound, -m = main display only
```

### 2. Downscale to logical resolution

Retina displays capture at 2× physical pixels (e.g. 2880×1800 for a 1440×900 logical screen). Vision models are trained on typical screen resolutions and return coordinates in logical space. Sending the 2× image causes a 2× positional error. Pointer downscales before encoding:

```bash
sips --resampleWidth 1440 /tmp/pointer_tour_ss.png
# In-place resize; height adjusts proportionally
```

After this, AI coordinates map 1:1 to AppKit logical points. No scaling is applied to the returned coordinates.

### 3. Base64-encode and POST

The PNG is base64-encoded and sent as a `data:image/png;base64,...` URL in an `image_url` content block:

```rust
let body = serde_json::json!({
    "model": model,
    "messages": [
        { "role": "system", "content": SYSTEM_PROMPT },
        { "role": "user", "content": [
            { "type": "text", "text": user_context_string },
            { "type": "image_url", "image_url": {
                "url": format!("data:image/png;base64,{}", b64)
            }}
        ]}
    ],
    "temperature": 0,
    "max_tokens": 1024   // 4096 for reasoning models (qwen3, deepseek-r, o1…)
});
```

The body is written to `/tmp/pointer_body.json` and sent with `curl --data-binary @file` to avoid exceeding `ARG_MAX` for large screenshots.

### 4. Coordinate conversion

```
AI returns (x, y) in image pixel space  (origin top-left, y down)
         ↓ (no scaling needed — image == logical resolution after sips)
logical_x = x
logical_y = y
         ↓ (AppKit uses bottom-left origin)
ns_x = logical_x
ns_y = screen_height − logical_y
```

The dot is then animated from its current position to `(ns_x, ns_y)` using a smoothstep easing over 16 × 50 ms ticks.

### 5. Retry logic

Each call wraps `query_groq_once` in up to 3 attempts:

| Failure | Action |
|---|---|
| `rate_limit_exceeded` | Parse wait time from error body (`"try again in X.Xs"`), sleep, retry |
| Out-of-bounds coordinates | Inject scroll hint, retry |
| Model hit `max_tokens` (`content: null`, `finish_reason: "length"`) | Surface clear error, retry |
| JSON parse failure | Retry with same prompt |

**Stuck loop detection:** if the last 3 history entries share the same action, a `STUCK LOOP DETECTED` hint is injected into the next prompt. After 20 steps, a `MAX STEPS REACHED` hint forces `is_final=true`.

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
    {
      "role": "system",
      "content": "You are a macOS GUI tour-guide..."
    },
    {
      "role": "user",
      "content": [
        {
          "type": "text",
          "text": "app:    Brave Browser\nwindow: GitHub — pointer\nurl:    https://github.com/you/pointer\ngoal:   Star this repo\n\nScreenshot size: 1440 × 900 px  (origin TOP-LEFT, y DOWN)\nValid x range: 1 – 1439   Valid y range: 1 – 899\n\nSteps done so far:\nNone yet.\n\nIdentify the single next UI element and return its centre pixel."
        },
        {
          "type": "image_url",
          "image_url": {
            "url": "data:image/png;base64,<base64-encoded-screenshot>"
          }
        }
      ]
    }
  ],
  "temperature": 0,
  "max_tokens": 1024
}
```

**OpenRouter** additionally requires:
```http
HTTP-Referer: https://pointer.app
X-Title: Pointer
```

### Response

The model must return raw JSON (no markdown fences, no explanation):

```json
{
  "x": 1312,
  "y": 148,
  "action": "Click",
  "description": "Click the Star button in the top-right of the repository",
  "reason": "The Star button is visible next to Watch and Fork — clicking it will star the repo",
  "is_final": false
}
```

For informational goals where no interaction is needed:

```json
{
  "action": "Answer",
  "description": "The agent code is in src/agent.rs — it handles screenshot capture, the Groq API call, and coordinate parsing.",
  "reason": "The file tree is visible in the editor sidebar.",
  "is_final": true
}
```

`x` and `y` are optional when `is_final: true`.

### Response fields

| Field | Type | Required | Description |
|---|---|---|---|
| `x` | integer | No (default 0) | Horizontal coordinate in logical screen pixels, origin top-left |
| `y` | integer | No (default 0) | Vertical coordinate in logical screen pixels, origin top-left |
| `action` | string | Yes | `Click`, `Double-click`, `Right-click`, `Type`, `Scroll`, `Hover`, `Answer`, `Finished` |
| `description` | string | Yes | Shown in the step card. For final steps, this is the answer text |
| `reason` | string | No | Shown in smaller text below description |
| `is_final` | boolean | No (default false) | `true` = last step, triggers completion panel |

### System prompt

Pointer injects several rules into `SYSTEM_PROMPT` (`src/agent.rs`):

- **Coordinate rules** — origin top-left, stay within stated valid ranges
- **Scroll vs click** — only scroll if the target is completely off-screen
- **Loop / stuck rule** — stop repeating; try a different element or answer directly
- **Research / informational goals** — answer directly from the visible screenshot; never loop just to find "more detail"
- **History rule** — do not repeat already-completed steps

### UI-TARS models (self-hosted)

[UI-TARS](https://github.com/bytedance/UI-TARS) uses a `Thought: / Action:` format with 0–999 normalised coordinates:

```
Thought: The Star button is visible in the top-right.
Action: click(point='<point>[[910,102]]</point>')
```

Pointer auto-detects UI-TARS by model name and switches to `UITARS_SYSTEM_PROMPT` + a dedicated parser that converts `[[x,y]]` → image pixels:

```
pixel_x = (norm_x / 999.0) × img_width
pixel_y = (norm_y / 999.0) × img_height
```

Supported UI-TARS actions: `click`, `double_click`, `right_click`, `hover`, `type`, `scroll`, `finished`.

---

## Supported Models & Providers

Pointer requires a **vision-capable** model that accepts `image_url` content. It runs a 1×1 PNG vision probe during Test Connection to verify this.

### Provider 0 — Groq

Fast, free tier available. Default provider.

| Model | Notes |
|---|---|
| `meta-llama/llama-4-scout-17b-16e-instruct` | **Default** — best speed/accuracy balance |
| `meta-llama/llama-4-maverick-17b-128e-instruct` | Larger context window |

Get a key at [console.groq.com](https://console.groq.com).

### Provider 1 — OpenRouter

Access to hundreds of models via a single API key.

| Model | Notes |
|---|---|
| `meta-llama/llama-4-maverick:free` | Default — free tier |
| `google/gemini-2.0-flash-001` | Fast Gemini vision |
| `anthropic/claude-opus-4-5` | Highest accuracy |
| `qwen/qwen-vl-plus` | Strong vision, reasoning |

> Reasoning models (`qwen3`, `deepseek-r1`, `o1`, `o3`, `o4`, `r1`) are auto-detected and get `max_tokens: 4096` to allow the chain-of-thought to finish before writing the JSON answer.

Get a key at [openrouter.ai/keys](https://openrouter.ai/keys).

### Provider 2 — Local / Self-hosted

Any server implementing the OpenAI Chat Completions API with vision support.

| Server | Vision | Base URL |
|---|---|---|
| **vLLM** | Yes | `http://localhost:8000/v1` |
| **Ollama** | Yes (llava, llama3.2-vision) | `http://localhost:11434/v1` |
| **LM Studio** | Yes | `http://localhost:1234/v1` |
| **LocalAI** | Model-dependent | `http://localhost:8080/v1` |

Use `EMPTY` as the API key if your server requires no authentication.

**UI-TARS with vLLM:**
```bash
vllm serve bytedance-research/UI-TARS-7B-SFT \
  --dtype bfloat16 \
  --max-model-len 4096
```
Set model to `bytedance-research/UI-TARS-7B-SFT`, base URL to `http://localhost:8000/v1`.

---

## Configuration

### Config file — `~/.pointer_config.json`

Written on every Apply click. Loaded on startup (takes precedence over env vars).

```json
{
  "provider": 0,
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
| `openrouter_api_key` | OpenRouter API key (`sk-or-v1-...`) |
| `openrouter_model` | Model slug (any on [openrouter.ai/models](https://openrouter.ai/models)) |
| `openrouter_url` | OpenRouter completions endpoint (normally unchanged) |
| `local_base_url` | Base URL for local server (`/chat/completions` appended automatically) |
| `local_api_key` | Auth token; use `"EMPTY"` for servers with no auth |
| `local_model` | Model name as the local server expects it |

### Environment variables (defaults for new installs)

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
│                    NSWindow / NSPanel creation, startup bootstrap
├── config.rs        PointerConfig (serde JSON), load/save, MsPtrs
├── state.rs         All global statics (AtomicU8/U32/U64, OnceLock<Mutex<T>>),
│                    window accessor functions, dot colour, update_display()
├── tour.rs          State machine transitions (to_dot, to_input, to_loading,
│                    show_step, on_next_step_pressed, show_completion),
│                    context capture (NSWorkspace + AXUIElement)
├── events.rs        CGEventTap setup + callback, key processing,
│                    AXIsProcessTrusted accessibility check
├── agent.rs         TourAgent, next_step(), screenshot capture, sips resize,
│                    OpenAI/Groq/OpenRouter API call (curl), JSON parsing,
│                    UI-TARS parser, reasoning model detection
├── logger.rs        Daily-rotating log file (~/.pointer/logs/pointer_YYYY-MM-DD.log),
│                    log_info! / log_error! / log_ai! macros, pure-std UTC timestamps
└── ui/
    ├── mod.rs       PointerView (dot drawRect:), InputView (key capture),
    │                class registration, public re-exports
    ├── input.rs     Prompt bar panel (NSScrollView + NSTextField)
    ├── step.rs      Step guide panel (header, description, reason, button)
    ├── hint.rs      Action hint pill (frosted NSVisualEffectView)
    └── settings.rs  Colour picker panel (8 swatches),
                     Model Settings panel (NSPopUpButton + 3 provider boxes)
```

### Key types

```rust
// agent.rs
pub struct TourAgent {
    goal, app, window, url,  // context passed to AI each step
    screen_w, screen_h,      // logical screen dimensions
    api_key, api_url, model, // resolved from PointerConfig
    history: Vec<String>,    // "Step N — Action: description" entries
    step_num: usize,
}

pub struct AgentStep {
    x, y,         // logical screen coordinates (AppKit bottom-left origin)
    action,       // "Click" | "Scroll" | ...
    description,  // shown in step card
    reason,       // shown below description
    step_num,
    is_final,
}

// config.rs
pub struct PointerConfig {
    provider: u8,           // 0 | 1 | 2
    groq_api_key, groq_model, groq_url,
    openrouter_api_key, openrouter_model, openrouter_url,
    local_base_url, local_api_key, local_model,
}
```

---

## Building for Release

```bash
cargo build --release
# Binary: ./target/release/pointer  (~6 MB stripped)
```

**Run at login:**  
System Settings → General → Login Items → add `pointer`

**Code-sign (optional, for Gatekeeper):**
```bash
codesign --force --deep -s "Developer ID Application: Your Name" \
  ./target/release/pointer
```

---

## Logging

Pointer writes structured logs to `~/.pointer/logs/pointer_YYYY-MM-DD.log`. A new file is created automatically each day.

```
══════════════════════════════════════════
Pointer started  2026-04-20 11:13:41 UTC
Log file: /Users/you/.pointer/logs/pointer_2026-04-20.log
══════════════════════════════════════════
[2026-04-20 11:13:41 UTC] [INFO ] app:    Brave Browser
[2026-04-20 11:13:41 UTC] [INFO ] window: Runpod GPU Cloud - Brave
[2026-04-20 11:13:41 UTC] [INFO ] url:    https://console.runpod.io/deploy
[2026-04-20 11:13:41 UTC] [INFO ] query:  help me choose a GPU instance
[2026-04-20 11:13:41 UTC] [INFO ] screenshot 2880×1800 → 1440×900 sent to AI
[2026-04-20 11:13:45 UTC] [AI   ] → {"x":720,"y":450,"action":"Scroll",...}
```

Log levels: `INFO`, `ERROR`, `AI` (raw model response). All logs also print to stderr.

---

## Troubleshooting

**`✗ Could not connect to server`**
- Check your API key is set and valid
- Run **Test Connection** in Model Settings — the status label shows the exact API error

**`✗ HTTP 400 — model does not support image input`**
- The selected model is text-only. Switch to a vision model
- On OpenRouter, embedding models (e.g. `gemini-embedding-*`) return 400 — use a chat/vision model

**`✗ HTTP 400` (OpenRouter, no further detail)**
- OpenRouter requires `HTTP-Referer` and `X-Title` headers — these are sent automatically. If still failing, verify the model slug at openrouter.ai/models

**`model hit max_tokens mid-reasoning`**
- A reasoning model (qwen3, deepseek-r1, etc.) ran out of tokens. Pointer auto-allocates 4096 tokens for detected reasoning models. If still failing, try a non-reasoning model

**Hotkey `⌘⇧P` does nothing**
- Accessibility permission not granted — System Settings → Privacy & Security → Accessibility → add `pointer`
- CGEventTap is logging retries: check `~/.pointer/logs/pointer_YYYY-MM-DD.log`

**Dot clicks the wrong place**
- Ensure the target window is on your **primary display** (`screencapture -m` only captures the main display)
- On multi-monitor setups, `NSScreen.screens[0]` is the primary display

**AI loops on the same action**
- Loop detection fires after 3 identical consecutive actions (see `STUCK LOOP DETECTED` in the prompt)
- A 20-step hard cap forces a final answer
- If looping persists, try a larger or different model with better instruction-following

**Step card appears in the wrong spot**
- The card positions itself above the target element when there is space, or below if not. On very small screens it may overlap the target — this is a known limitation

---

## License

MIT
