use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::Deserialize;
use std::io::Read;
use std::process::Command;

const GROQ_MODEL_DEFAULT: &str = "meta-llama/llama-4-maverick-17b-128e-instruct";
const GROQ_URL_DEFAULT:   &str = "https://api.groq.com/openai/v1/chat/completions";

fn groq_api_key() -> String {
    std::env::var("GROQ_API_KEY")
        .expect("GROQ_API_KEY environment variable is not set")
}

fn groq_model() -> String {
    std::env::var("GROQ_MODEL").unwrap_or_else(|_| GROQ_MODEL_DEFAULT.to_owned())
}

fn groq_url() -> String {
    std::env::var("GROQ_URL").unwrap_or_else(|_| GROQ_URL_DEFAULT.to_owned())
}

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AgentStep {
    pub x:           f64,
    pub y:           f64,   // NS bottom-left origin
    pub action:      String,
    pub description: String,
    pub step_num:    usize,
    pub is_final:    bool,
}

// ── Browser URL via AppleScript ───────────────────────────────────────────────

pub fn get_browser_url(app: &str) -> Option<String> {
    let script = match app {
        "Brave Browser" =>
            r#"tell application "Brave Browser" to return URL of active tab of front window"#,
        "Google Chrome" | "Chromium" | "Chrome" =>
            r#"tell application "Google Chrome" to return URL of active tab of front window"#,
        "Safari" | "Safari Technology Preview" =>
            r#"tell application "Safari" to return URL of current tab of front window"#,
        "Arc" =>
            r#"tell application "Arc" to return URL of active tab of front window"#,
        _ => return None,
    };
    let out = Command::new("osascript").args(["-e", script]).output().ok()?;
    let url = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if url.is_empty() || url.starts_with("Error") { None } else { Some(url) }
}

// ── Tour agent ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct TourAgent {
    pub goal:     String,
    pub app:      String,
    pub window:   String,
    pub url:      Option<String>,
    pub screen_w: f64,
    pub screen_h: f64,
    history:      Vec<String>,
    step_num:     usize,
}

impl TourAgent {
    pub fn new(
        goal: String, app: String, window: String,
        url: Option<String>, screen_w: f64, screen_h: f64,
    ) -> Self {
        Self { goal, app, window, url, screen_w, screen_h, history: Vec::new(), step_num: 0 }
    }

    pub fn next_step(&mut self) -> Result<AgentStep, String> {
        let (b64, img_w, img_h) = capture_screenshot(
            self.screen_w as u32, self.screen_h as u32,
        )?;

        let raw = query_groq(
            &self.goal, &self.app, &self.window, self.url.as_deref(),
            &self.history, &b64, img_w as f64, img_h as f64,
        )?;

        self.step_num += 1;
        let n = self.step_num;

        let lx = raw.x * (self.screen_w / img_w as f64);
        let ly = raw.y * (self.screen_h / img_h as f64);
        let ns_y = self.screen_h - ly;

        let step = AgentStep {
            x: lx, y: ns_y,
            action:      raw.action.clone(),
            description: raw.description.clone(),
            step_num: n,
            is_final: raw.is_final,
        };
        self.history.push(format!("Step {n} — {}: {}", raw.action, raw.description));
        Ok(step)
    }
}

/// Run `work` on a background thread; dispatch the result to the GCD main queue.
pub fn spawn<W, R>(work: W, on_done: impl Fn(R) + Send + 'static)
where
    W: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    std::thread::spawn(move || {
        let result = work();
        dispatch::Queue::main().exec_async(move || on_done(result));
    });
}

// ── Screenshot ────────────────────────────────────────────────────────────────

fn capture_screenshot(logical_w: u32, logical_h: u32) -> Result<(String, u32, u32), String> {
    const PATH: &str = "/tmp/pointer_tour_ss.png";

    Command::new("screencapture")
        .args(["-x", "-m", "-t", "png", PATH])
        .status()
        .map_err(|e| format!("screencapture: {e}"))?;

    Command::new("sips")
        .args(["-z", &logical_h.to_string(), &logical_w.to_string(), PATH])
        .status()
        .map_err(|e| format!("sips resize: {e}"))?;

    let info = Command::new("sips")
        .args(["-g", "pixelWidth", "-g", "pixelHeight", PATH])
        .output()
        .map_err(|e| format!("sips info: {e}"))?;
    let info_s = String::from_utf8_lossy(&info.stdout);
    let actual_w = parse_sips_dim(&info_s, "pixelWidth").unwrap_or(logical_w);
    let actual_h = parse_sips_dim(&info_s, "pixelHeight").unwrap_or(logical_h);

    eprintln!("[pointer] screenshot {actual_w}×{actual_h} (logical {logical_w}×{logical_h})");

    let mut bytes = Vec::new();
    std::fs::File::open(PATH)
        .map_err(|e| format!("open screenshot: {e}"))?
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read screenshot: {e}"))?;

    Ok((B64.encode(&bytes), actual_w, actual_h))
}

fn parse_sips_dim(output: &str, key: &str) -> Option<u32> {
    output.lines()
        .find(|l| l.trim_start().starts_with(key))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse().ok())
}

// ── Groq API ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)] struct GroqResp   { choices: Vec<GroqChoice> }
#[derive(Deserialize)] struct GroqChoice { message: GroqMsg }
#[derive(Deserialize)] struct GroqMsg    { content: String }

#[derive(Deserialize)]
struct StepJson {
    x: f64,
    y: f64,
    action: String,
    description: String,
    #[serde(default)]
    is_final: bool,
}

/// Retry up to 3 times; accept only in-bounds, non-origin coordinates.
fn query_groq(
    goal: &str, app: &str, window: &str, url: Option<&str>,
    history: &[String], screenshot_b64: &str,
    img_w: f64, img_h: f64,
) -> Result<StepJson, String> {
    for attempt in 0..3u8 {
        match query_groq_once(goal, app, window, url, history, screenshot_b64, img_w, img_h) {
            Ok(s) if s.x > 1.0 && s.y > 1.0 && s.x < img_w - 1.0 && s.y < img_h - 1.0 => {
                return Ok(s);
            }
            Ok(s) => eprintln!(
                "[pointer] attempt {}: suspect coords ({:.0},{:.0}) — retrying",
                attempt + 1, s.x, s.y
            ),
            Err(e) if attempt < 2 => eprintln!("[pointer] attempt {}: {e} — retrying", attempt + 1),
            Err(e) => return Err(e),
        }
    }
    Err("Could not get valid screen coordinates after 3 attempts".into())
}

fn query_groq_once(
    goal: &str, app: &str, window: &str, url: Option<&str>,
    history: &[String], screenshot_b64: &str,
    img_w: f64, img_h: f64,
) -> Result<StepJson, String> {
    let history_text = if history.is_empty() {
        "None yet.".to_owned()
    } else {
        history.iter().enumerate()
            .map(|(i, s)| format!("{}. {s}", i + 1))
            .collect::<Vec<_>>().join("\n")
    };

    let url_line = url.map(|u| format!("url:    {u}\n")).unwrap_or_default();

    let user_text = format!(
        "app:    {app}\n\
         window: {window}\n\
         {url_line}\
         goal:   {goal}\n\
         \n\
         Screenshot size: {img_w:.0} × {img_h:.0} px  (origin TOP-LEFT, y DOWN)\n\
         Valid x range: 1 – {:.0}   Valid y range: 1 – {:.0}\n\
         \n\
         Steps done so far:\n\
         {history_text}\n\
         \n\
         Identify the single next UI element and return its centre pixel.",
        img_w - 1.0, img_h - 1.0
    );

    let body = serde_json::json!({
        "model": groq_model(),
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            {
                "role": "user",
                "content": [
                    { "type": "text",      "text": user_text },
                    { "type": "image_url", "image_url": {
                        "url": format!("data:image/png;base64,{screenshot_b64}")
                    }}
                ]
            }
        ],
        "temperature": 0,
        "max_tokens": 256
    }).to_string();

    let url = groq_url();
    let key = groq_api_key();
    let out = Command::new("curl")
        .args([
            "-s", "-X", "POST", &url,
            "-H", "Content-Type: application/json",
            "-H", &format!("Authorization: Bearer {key}"),
            "-d", &body,
        ])
        .output()
        .map_err(|e| format!("curl: {e}"))?;

    let raw = String::from_utf8_lossy(&out.stdout);

    let resp: GroqResp = serde_json::from_str(&raw)
        .map_err(|e| format!("Groq parse error: {e}\n---\n{raw}"))?;

    let content = resp.choices.into_iter().next()
        .ok_or("Groq: no choices")?
        .message.content;

    eprintln!("[pointer] AI → {content}");

    // Strip optional ```json … ``` fences.
    let json_str = content.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    serde_json::from_str::<StepJson>(json_str)
        .map_err(|e| format!("step JSON parse: {e}\n---\n{content}"))
}

// ── System prompt ─────────────────────────────────────────────────────────────

const SYSTEM_PROMPT: &str = r#"You are a macOS GUI tour-guide. Each turn you receive a screenshot and must identify the single next UI element the user should interact with.

COORDINATE SYSTEM
  • Origin (0,0) = TOP-LEFT corner of the image.
  • x increases rightward; y increases DOWNWARD.
  • The image dimensions and valid coordinate ranges are stated in the user message.
  • Return the CENTRE pixel of the target element.
  • x and y must both be strictly within the stated valid ranges.

HOW TO RESPOND
  1. Read the goal and steps already completed.
  2. Determine what the next action must be.
  3. Find that exact element in the screenshot — look at its actual pixel position.
  4. Before writing coordinates, mentally verify: is the element visible? Are x and y inside bounds?
  5. Write descriptions in plain English for non-technical users (e.g. "Click the blue Sign In button near the top right").

OUTPUT — raw JSON only, no markdown, no explanation:
{"x":<int>,"y":<int>,"action":"<Click|Type|Double-click|Right-click|Scroll|Hover>","description":"<one sentence>","is_final":<true|false>}"#;
