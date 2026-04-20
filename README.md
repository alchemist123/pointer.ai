# Pointer AI

**Pointer** is a macOS floating AI tour-guide that lives on top of every app on your screen. Press a hotkey, type a goal in plain English, and Pointer will guide you step by step — showing exactly where to click, type, or scroll — using a live screenshot fed to a vision-language model.

No browser extension. No app integration. It works on any macOS app, any website, any window.

---

## How It Works

```
User presses ⌘⇧P
      │
      ▼
Floating dot appears at cursor
      │
      ▼
User clicks dot → types goal ("Deploy my app on RunPod")
      │
      ▼
Pointer takes a screenshot + sends it to the vision AI
      │
      ▼
AI returns: next action, coordinates, reason, description
      │
      ▼
Animated dot flies to the target element
Step card shows what to do and why
      │
      ▼
User performs the action → clicks "I did it →"
      │
      ▼
Loop until AI sets is_final=true
      │
      ▼
Completion card shows full conclusion / answer
```

---

## Features

- **Universal** — works on any macOS app, browser, or native window
- **Vision AI** — sends full-resolution Retina screenshots to the model for pixel-accurate targeting
- **Step-by-step guide** — each step shows what to do, where to click, and why
- **Smart loop detection** — detects when the AI is stuck and forces a resolution or new approach
- **Scroll awareness** — knows when elements are off-screen and scrolls before clicking
- **Research mode** — for informational goals, gives a direct answer without unnecessary navigation
- **Hotkey toggle** — `⌘⇧P` shows/hides the overlay from any app
- **Local model support** — works with any OpenAI-compatible local endpoint (Ollama, LM Studio, vLLM, etc.)
- **Customisable dot** — 8 dot colours, dark frosted-glass UI

---

## Requirements

| Requirement | Detail |
|---|---|
| macOS | 12 Monterey or later (uses `NSVisualEffectView`, `CGEventTap`) |
| Rust | 1.70+ (`cargo build`) |
| Accessibility permission | Required for global hotkey + auto-advance on click |
| Vision model API | Groq (default) or any OpenAI-compatible local endpoint |

---

## Quick Start

### 1. Clone and build

```bash
git clone https://github.com/yourname/pointer.git
cd pointer
cargo build --release
```

### 2. Set your API key

```bash
cp .env.example .env
# Edit .env and add your Groq key:
#   GROQ_API_KEY=gsk_...
```

Source the env before running, or set the key in the app's Model Settings panel.

### 3. Run

```bash
source .env
./target/release/pointer
```

### 4. Grant Accessibility permission

On first launch, macOS will prompt you to grant Accessibility access. Go to:

**System Settings → Privacy & Security → Accessibility** → add `pointer`

This is required for the global `⌘⇧P` hotkey and the click auto-advance feature.

---

## Usage

| Action | How |
|---|---|
| Show/hide overlay | `⌘⇧P` from any app |
| Open prompt | Click the floating dot |
| Submit goal | Type your goal, press `Return` |
| Dismiss | Press `Escape` at any time |
| Next step | Click `"I did it →"` or click the actual target element |
| Change dot colour | Click `⚙` → Pointer Colour |
| Change model/API | Click `⚙` → Model Settings |

---

## Configuration

### Environment variables

```bash
# Required for Groq
GROQ_API_KEY=gsk_...

# Optional overrides
GROQ_MODEL=meta-llama/llama-4-scout-17b-16e-instruct
GROQ_URL=https://api.groq.com/openai/v1/chat/completions
```

Settings are also saved to `~/.pointer_config.json` when you click Apply in the Model Settings panel. The JSON file takes precedence over environment variables on subsequent launches.

### Config file format

```json
{
  "provider": 0,
  "groq_api_key": "gsk_...",
  "groq_model": "meta-llama/llama-4-scout-17b-16e-instruct",
  "groq_url": "https://api.groq.com/openai/v1/chat/completions",
  "local_base_url": "",
  "local_api_key": "EMPTY",
  "local_model": ""
}
```

| Field | Description |
|---|---|
| `provider` | `0` = Groq, `1` = Local / self-hosted |
| `groq_api_key` | Your Groq API key (`gsk_...`) |
| `groq_model` | Vision-capable model name on Groq |
| `groq_url` | Groq chat completions endpoint |
| `local_base_url` | Base URL for your local server (e.g. `http://localhost:11434/v1`) |
| `local_api_key` | API key for local server — use `"EMPTY"` if none required |
| `local_model` | Model name as the local server expects it |

---

## API Reference

Pointer uses the **OpenAI-compatible Chat Completions API** with **vision (image_url) support**.

### Request format

Every step sends a `POST` to the configured endpoint:

```json
{
  "model": "<model-name>",
  "messages": [
    {
      "role": "system",
      "content": "<system prompt>"
    },
    {
      "role": "user",
      "content": [
        {
          "type": "text",
          "text": "app: Brave Browser\nwindow: GitHub\ngoal: Star this repo\n\nScreenshot size: 2880 × 1800 px  (origin TOP-LEFT, y DOWN)\nValid x range: 1 – 2879   Valid y range: 1 – 1799\n\nSteps done so far:\nNone yet.\n\nIdentify the single next UI element and return its centre pixel."
        },
        {
          "type": "image_url",
          "image_url": {
            "url": "data:image/png;base64,<base64-screenshot>"
          }
        }
      ]
    }
  ],
  "temperature": 0,
  "max_tokens": 512
}
```

### Response format

The model must return raw JSON (no markdown fences):

```json
{
  "x": 1423,
  "y": 312,
  "action": "Click",
  "description": "Click the Star button in the top-right area of the repository page",
  "reason": "The Star button is visible in the toolbar next to Watch and Fork — clicking it will star the repo",
  "is_final": false
}
```

| Field | Type | Description |
|---|---|---|
| `x` | integer | Horizontal pixel coordinate (origin = top-left of screenshot) |
| `y` | integer | Vertical pixel coordinate (origin = top-left of screenshot) |
| `action` | string | One of: `Click`, `Double-click`, `Right-click`, `Type`, `Scroll`, `Hover` |
| `description` | string | Human-readable instruction shown in the step card |
| `reason` | string | Why this is the right next step (shown in smaller text below description) |
| `is_final` | boolean | `true` on the last step — triggers the completion panel |

### Coordinate system

```
(0,0) ─────────────────────── x →
  │
  │   Screenshot (full Retina resolution, e.g. 2880×1800)
  │
  y ↓
```

Pointer automatically scales AI coordinates from screenshot pixels to logical screen points:

```
logical_x = ai_x × (screen_width  / screenshot_width)
logical_y = ai_y × (screen_height / screenshot_height)
```

For a standard Retina display (2× scale), this halves the coordinates. The animated dot is then placed at `(logical_x, screen_height − logical_y)` to convert from top-left origin to macOS's bottom-left NSWindow origin.

---

## Supported Models

Pointer requires a **vision-capable** model that can process `image_url` content. It runs a vision probe during Test Connection to verify this before starting a tour.

### Groq (recommended)

| Model | Notes |
|---|---|
| `meta-llama/llama-4-scout-17b-16e-instruct` | **Default — best balance of speed and accuracy** |
| `meta-llama/llama-4-maverick-17b-128e-instruct` | Larger context, check Groq availability |

Get a free API key at [console.groq.com](https://console.groq.com).

### Local / self-hosted

Any server that implements the OpenAI Chat Completions API with vision support:

| Server | Vision support | Config example |
|---|---|---|
| **Ollama** | Yes (llava, llama3.2-vision) | `http://localhost:11434/v1` |
| **LM Studio** | Yes | `http://localhost:1234/v1` |
| **vLLM** | Yes | `http://localhost:8000/v1` |
| **LocalAI** | Depends on model | `http://localhost:8080/v1` |

Set `provider` to `1` in settings. Use `EMPTY` as the API key if your local server requires no authentication.

---

## Project Structure

```
src/
  main.rs        Entry point, AppController ObjC class, window setup
  config.rs      PointerConfig struct, JSON persistence, MsPtrs
  state.rs       All global statics, constants, window accessors, helpers
  tour.rs        State machine — dot/input/loading/step/completion transitions
  events.rs      CGEventTap, global hotkey, keyboard processing, accessibility
  agent.rs       TourAgent, Groq API calls, screenshot capture, system prompt
  ui/
    mod.rs       PointerView (animated dot), InputView (key capture)
    input.rs     Prompt bar panel
    step.rs      Step guide panel
    hint.rs      Action hint pill near the dot
    settings.rs  Colour picker panel, Model Settings panel
```

### State machine

```
0 hidden  ──⌘⇧P──▶  1 dot  ──click──▶  2 input  ──Return──▶  3 loading
                                                                    │
    ◀──Escape──────────────────────────────────────────────────────┘
                                                              AI responds
                                                                    │
                                                                    ▼
                                                              4 step shown
                                                         ┌──────────────────┐
                                                         │  "I did it →"    │
                                                         │  or click target │
                                                         └──────┬───────────┘
                                                    is_final=false │  is_final=true
                                                         back to 3 │       │
                                                                    │       ▼
                                                                    │   5 done (auto-dismiss ~5s)
                                                                    │       │
                                                                    └───────▼
                                                                    0 hidden
```

---

## Building for Release

```bash
cargo build --release
# Binary at: ./target/release/pointer
```

To run at login, add the binary to **System Settings → General → Login Items**.

---

## Troubleshooting

**"Could not connect to server"**
- Check your API key is correct and starts with `gsk_`
- Verify the model name — Groq removes models periodically. Use `meta-llama/llama-4-scout-17b-16e-instruct`
- Run Test Connection in Model Settings to see the exact error message

**"Model does not support vision/images"**
- The selected model is text-only. Switch to a vision model (see Supported Models above)
- Pointer's Test Connection performs a vision probe and will catch this before a tour starts

**Hotkey does not work**
- Accessibility permission is not granted. Add `pointer` in System Settings → Privacy & Security → Accessibility

**Dot appears at wrong position**
- This can happen on multi-monitor setups. Pointer uses the primary display (`NSScreen.screens[0]`) and `screencapture -m` (main display only). Ensure the target window is on your primary display

**AI keeps repeating the same action**
- Loop detection is built in: after 3 identical consecutive actions, Pointer injects a strong hint to break the loop or answer directly
- A hard cap of 20 steps forces a final answer if the tour runs too long

---

## License

MIT
