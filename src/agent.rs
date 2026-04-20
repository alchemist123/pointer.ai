use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::Deserialize;
use std::io::Read;
use std::process::Command;

use crate::{log_ai, log_error, log_info};

pub const GROQ_MODEL_DEFAULT:    &str = "meta-llama/llama-4-scout-17b-16e-instruct";
pub const GROQ_URL_DEFAULT:      &str = "https://api.groq.com/openai/v1/chat/completions";
pub const UITARS_MODEL_EXAMPLE:  &str = "bytedance-research/UI-TARS-7B-SFT";
pub const UITARS_URL_EXAMPLE:    &str = "http://localhost:8000/v1/chat/completions";

// ── UI-TARS detection ─────────────────────────────────────────────────────────

fn is_uitars_model(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("ui-tars") || m.contains("uitars")
}

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AgentStep {
    pub x:           f64,
    pub y:           f64,
    pub action:      String,
    pub description: String,
    pub reason:      String,
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
    pub api_key:  String,
    pub api_url:  String,
    pub model:    String,
    history:      Vec<String>,
    step_num:     usize,
}

impl TourAgent {
    pub fn new(
        goal: String, app: String, window: String, url: Option<String>,
        screen_w: f64, screen_h: f64,
        api_key: String, api_url: String, model: String,
    ) -> Self {
        Self { goal, app, window, url, screen_w, screen_h,
               api_key, api_url, model, history: Vec::new(), step_num: 0 }
    }

    pub fn next_step(&mut self) -> Result<AgentStep, String> {
        let (b64, img_w, img_h) = capture_screenshot(self.screen_w as u32, self.screen_h as u32)?;
        let raw = query_groq(
            &self.goal, &self.app, &self.window, self.url.as_deref(),
            &self.history, &b64, img_w as f64, img_h as f64,
            &self.api_key, &self.api_url, &self.model,
        )?;
        self.step_num += 1;
        let n = self.step_num;
        let lx   = raw.x * (self.screen_w / img_w as f64);
        let ly   = raw.y * (self.screen_h / img_h as f64);
        let ns_y = self.screen_h - ly;
        let step = AgentStep {
            x: lx, y: ns_y,
            action: raw.action.clone(), description: raw.description.clone(),
            reason: raw.reason.clone(),
            step_num: n, is_final: raw.is_final,
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

/// Quick connectivity check — returns Ok or a short human-readable error.
pub fn test_connection(api_key: &str, model: &str, url: &str) -> Result<(), String> {
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role":"user","content":"hi"}],
        "max_tokens": 1,
        "temperature": 0
    }).to_string();

    // Send a plain-text message first to verify auth + endpoint.
    // Then send a vision probe to check image support.
    const TEST_PATH: &str = "/tmp/pointer_test_body.json";
    std::fs::write(TEST_PATH, &body).map_err(|e| format!("✗ write body: {e}"))?;
    let out = Command::new("curl")
        .args([
            "-s", "-w", "\n%{http_code}",
            "--connect-timeout", "8",
            "-X", "POST", url,
            "-H", "Content-Type: application/json",
            "-H", &format!("Authorization: Bearer {api_key}"),
            "--data-binary", &format!("@{TEST_PATH}"),
        ])
        .output()
        .map_err(|e| format!("✗ {e}"))?;

    let raw = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Last line is the HTTP status code (from -w "\n%{http_code}").
    let (resp_body, status_line) = raw.rsplit_once('\n').unwrap_or(("", raw.trim()));
    let code = status_line.trim().parse::<u16>().unwrap_or(0);

    match code {
        401 | 403 => return Err("✗ Invalid API key".into()),
        404       => return Err("✗ Model not found — update model name".into()),
        0         => {
            let detail = stderr.trim();
            return Err(if detail.is_empty() {
                "✗ Could not connect to server".into()
            } else {
                format!("✗ {}", &detail[..detail.len().min(80)])
            });
        }
        c if c != 200 && c != 201 => return Err(format!("✗ HTTP {c}")),
        _ => {}
    }

    // Now probe vision support with a 1×1 transparent PNG.
    let vision_body = serde_json::json!({
        "model": model,
        "messages": [{"role":"user","content":[
            {"type":"text","text":"reply with one word"},
            {"type":"image_url","image_url":{"url":
                "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAC0lEQVQI12NgAAIABQAABjE+ibYAAAAASUVORK5CYII="
            }}
        ]}],
        "max_tokens": 1,
        "temperature": 0
    }).to_string();
    std::fs::write(TEST_PATH, &vision_body).map_err(|e| format!("✗ write body: {e}"))?;
    let vout = Command::new("curl")
        .args([
            "-s", "--connect-timeout", "8",
            "-X", "POST", url,
            "-H", "Content-Type: application/json",
            "-H", &format!("Authorization: Bearer {api_key}"),
            "--data-binary", &format!("@{TEST_PATH}"),
        ])
        .output()
        .map_err(|e| format!("✗ {e}"))?;
    let vraw = String::from_utf8_lossy(&vout.stdout);
    if vraw.contains("must be a string") || vraw.contains("does not support") {
        return Err(format!(
            "✗ '{model}' is text-only — use a vision model e.g. meta-llama/llama-4-scout-17b-16e-instruct"
        ));
    }

    let _ = resp_body; // text check passed
    Ok(())
}

// ── Screenshot ────────────────────────────────────────────────────────────────

fn capture_screenshot(logical_w: u32, logical_h: u32) -> Result<(String, u32, u32), String> {
    const PATH: &str = "/tmp/pointer_tour_ss.png";
    Command::new("screencapture").args(["-x", "-m", "-t", "png", PATH])
        .status().map_err(|e| format!("screencapture: {e}"))?;
    // Don't downscale — send the native resolution (Retina = 2x) to the AI so
    // it has maximum pixel detail for accurate element targeting. The coordinate
    // scaling in next_step handles the physical→logical conversion via
    // raw.x * (screen_w / img_w).
    let info = Command::new("sips").args(["-g", "pixelWidth", "-g", "pixelHeight", PATH])
        .output().map_err(|e| format!("sips info: {e}"))?;
    let info_s = String::from_utf8_lossy(&info.stdout);
    let actual_w = parse_sips_dim(&info_s, "pixelWidth").unwrap_or(logical_w);
    let actual_h = parse_sips_dim(&info_s, "pixelHeight").unwrap_or(logical_h);
    log_info!("screenshot {actual_w}×{actual_h} (logical {logical_w}×{logical_h})");
    let mut bytes = Vec::new();
    std::fs::File::open(PATH).map_err(|e| format!("open screenshot: {e}"))?
        .read_to_end(&mut bytes).map_err(|e| format!("read screenshot: {e}"))?;
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
    x: f64, y: f64,
    action: String, description: String,
    #[serde(default)] reason: String,
    #[serde(default)] is_final: bool,
}

// ── UI-TARS response parser ───────────────────────────────────────────────────

fn parse_uitars_response(content: &str, img_w: f64, img_h: f64) -> Result<StepJson, String> {
    // Expected format:
    //   Thought: <reasoning>
    //   Action: click(point='<point>[[x,y]]</point>')
    // Coordinates are 0-999 normalized; convert to image pixels.

    let action_line = content.lines()
        .find(|l| l.trim_start().starts_with("Action:"))
        .ok_or_else(|| format!("UI-TARS: no Action line in:\n{content}"))?
        .trim_start_matches("Action:").trim().to_owned();

    let thought = content.lines()
        .find(|l| l.trim_start().starts_with("Thought:"))
        .map(|l| l.trim_start_matches("Thought:").trim().to_owned())
        .unwrap_or_default();

    // finished(content='...')
    if let Some(rest) = action_line.strip_prefix("finished(") {
        let msg = extract_kwarg(rest, "content").unwrap_or_else(|| action_line.clone());
        return Ok(StepJson {
            x: img_w / 2.0, y: img_h / 2.0,
            action: "Finished".into(),
            description: msg,
            reason: thought,
            is_final: true,
        });
    }

    // Parse optional point= from click/scroll/drag/hover
    let (nx, ny) = parse_point_arg(&action_line).unwrap_or((500.0, 500.0));
    let px = (nx / 999.0) * img_w;
    let py = (ny / 999.0) * img_h;

    if action_line.starts_with("click(") || action_line.starts_with("left_click(") {
        return Ok(StepJson {
            x: px, y: py,
            action: "Click".into(),
            description: format!("Click at ({nx:.0},{ny:.0})"),
            reason: thought,
            is_final: false,
        });
    }
    if action_line.starts_with("double_click(") {
        return Ok(StepJson {
            x: px, y: py,
            action: "Double-click".into(),
            description: format!("Double-click at ({nx:.0},{ny:.0})"),
            reason: thought,
            is_final: false,
        });
    }
    if action_line.starts_with("right_click(") {
        return Ok(StepJson {
            x: px, y: py,
            action: "Right-click".into(),
            description: format!("Right-click at ({nx:.0},{ny:.0})"),
            reason: thought,
            is_final: false,
        });
    }
    if let Some(rest) = action_line.strip_prefix("type(") {
        let text = extract_kwarg(rest, "content").unwrap_or_default();
        return Ok(StepJson {
            x: px, y: py,
            action: "Type".into(),
            description: text,
            reason: thought,
            is_final: false,
        });
    }
    if action_line.starts_with("scroll(") {
        let dir = extract_kwarg(&action_line, "direction").unwrap_or_else(|| "down".into());
        let (sx, sy) = match dir.as_str() {
            "up"    => (img_w / 2.0, img_h * 0.15),
            "left"  => (img_w * 0.15, img_h / 2.0),
            "right" => (img_w * 0.85, img_h / 2.0),
            _       => (img_w / 2.0, img_h * 0.85), // down
        };
        return Ok(StepJson {
            x: if nx != 500.0 { px } else { sx },
            y: if ny != 500.0 { py } else { sy },
            action: "Scroll".into(),
            description: format!("Scroll {dir}"),
            reason: thought,
            is_final: false,
        });
    }
    if action_line.starts_with("hover(") {
        return Ok(StepJson {
            x: px, y: py,
            action: "Hover".into(),
            description: format!("Hover at ({nx:.0},{ny:.0})"),
            reason: thought,
            is_final: false,
        });
    }

    Err(format!("UI-TARS: unrecognised action: {action_line}"))
}

fn parse_point_arg(s: &str) -> Option<(f64, f64)> {
    // Matches <point>[[x,y]]</point> or [[x,y]]
    let start = s.find("[[")? + 2;
    let end   = s[start..].find("]]")?;
    let inner = &s[start..start+end];
    let mut it = inner.split(',');
    let x: f64 = it.next()?.trim().parse().ok()?;
    let y: f64 = it.next()?.trim().parse().ok()?;
    Some((x, y))
}

fn extract_kwarg(s: &str, key: &str) -> Option<String> {
    // Find key='...' or key="..."
    let needle = format!("{key}=");
    let pos = s.find(&needle)?;
    let rest = &s[pos + needle.len()..];
    let quote = rest.chars().next()?;
    if quote == '\'' || quote == '"' {
        let inner = &rest[1..];
        let end = inner.find(quote)?;
        Some(inner[..end].to_owned())
    } else {
        // unquoted value up to next comma or )
        let end = rest.find([',', ')']).unwrap_or(rest.len());
        Some(rest[..end].trim().to_owned())
    }
}

fn query_groq(
    goal: &str, app: &str, window: &str, url: Option<&str>,
    history: &[String], screenshot_b64: &str, img_w: f64, img_h: f64,
    api_key: &str, api_url: &str, model: &str,
) -> Result<StepJson, String> {
    // Detect loop: if the last 3 history entries share the same action keyword,
    // the tour is stuck — inject a strong intervention hint.
    let stuck_hint = if history.len() >= 3 {
        let tail = &history[history.len()-3..];
        // Extract the action word (between "— " and ":") from each entry.
        let actions: Vec<&str> = tail.iter()
            .filter_map(|s| s.split("— ").nth(1)?.split(':').next())
            .collect();
        if actions.len() == 3 && actions[0] == actions[1] && actions[1] == actions[2] {
            format!(
                "\n\nSTUCK LOOP DETECTED: You have repeated the action '{}' 3 times with no \
                 progress. STOP repeating this action. You have two options:\n\
                 1. If the goal is informational (comparing, finding info), look at the CURRENT \
                    screenshot and answer the question directly — set is_final=true and put your \
                    answer in description.\n\
                 2. If navigation is truly needed, try a COMPLETELY DIFFERENT element or approach \
                    than what you have been clicking.",
                actions[0]
            )
        } else { String::new() }
    } else { String::new() };

    // Hard limit: if > 20 steps already done, force a final answer.
    let max_hint = if history.len() >= 20 {
        "\n\nMAX STEPS REACHED: You must now set is_final=true and give a complete \
         answer/summary based on what you have seen so far. Do not take any more actions.".to_owned()
    } else { String::new() };

    let base_extra = format!("{stuck_hint}{max_hint}");

    let oob_hint = format!(
        "\n\nWARNING: Your last response had coordinates outside the valid range \
         (x: 1–{:.0}, y: 1–{:.0}). The target element is NOT visible in this screenshot. \
         Do NOT guess or extrapolate. Instead return action=\"Scroll\" with coordinates \
         pointing to the BOTTOM-CENTRE of the visible page ({:.0}, {:.0}) so the user \
         can scroll down to reveal hidden content.",
        img_w - 1.0, img_h - 1.0,
        img_w / 2.0, img_h * 0.85,
    );
    let mut extra = base_extra.clone();
    for attempt in 0..3u8 {
        match query_groq_once(goal, app, window, url, history, screenshot_b64,
                              img_w, img_h, api_key, api_url, model, &extra) {
            Ok(s) if s.x > 1.0 && s.y > 1.0 && s.x < img_w-1.0 && s.y < img_h-1.0 => return Ok(s),
            Ok(s) => {
                log_info!("attempt {}: out-of-bounds ({:.0},{:.0}) — retrying with scroll hint",
                          attempt+1, s.x, s.y);
                extra = format!("{base_extra}{oob_hint}");
            }
            Err(e) if attempt < 2 => log_error!("attempt {}: {e} — retrying", attempt+1),
            Err(e) => return Err(e),
        }
    }
    Ok(StepJson {
        x: img_w / 2.0, y: img_h * 0.85,
        action: "Scroll".into(),
        description: "Scroll down to reveal more content".into(),
        reason: "The next element is not visible — scroll to bring it into view.".into(),
        is_final: false,
    })
}

fn parse_retry_after(raw: &str) -> Option<f64> {
    let pos = raw.find("try again in ")?;
    let rest = &raw[pos + "try again in ".len()..];
    let end = rest.find('s')?;
    rest[..end].trim().parse().ok()
}

fn query_groq_once(
    goal: &str, app: &str, window: &str, url: Option<&str>,
    history: &[String], screenshot_b64: &str, img_w: f64, img_h: f64,
    api_key: &str, api_url: &str, model: &str, extra_hint: &str,
) -> Result<StepJson, String> {
    let uitars = is_uitars_model(model);
    let history_text = if history.is_empty() { "None yet.".to_owned() } else {
        history.iter().enumerate().map(|(i,s)| format!("{}. {s}", i+1)).collect::<Vec<_>>().join("\n")
    };
    let url_line = url.map(|u| format!("url:    {u}\n")).unwrap_or_default();

    let (sys_prompt, user_text, max_tokens) = if uitars {
        let text = format!(
            "Task: {goal}\nApp: {app}\nWindow: {window}\n{url_line}\
             Steps done:\n{history_text}\n{extra_hint}"
        );
        (UITARS_SYSTEM_PROMPT, text, 256usize)
    } else {
        let text = format!(
            "app:    {app}\nwindow: {window}\n{url_line}goal:   {goal}\n\n\
             Screenshot size: {img_w:.0} × {img_h:.0} px  (origin TOP-LEFT, y DOWN)\n\
             Valid x range: 1 – {:.0}   Valid y range: 1 – {:.0}\n\n\
             Steps done so far:\n{history_text}\n\n\
             Identify the single next UI element and return its centre pixel.{extra_hint}",
            img_w-1.0, img_h-1.0
        );
        (SYSTEM_PROMPT, text, 512usize)
    };

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role":"system","content": sys_prompt},
            {"role":"user","content":[
                {"type":"text","text": user_text},
                {"type":"image_url","image_url":{"url": format!("data:image/png;base64,{screenshot_b64}")}}
            ]}
        ],
        "temperature": 0,
        "max_tokens": max_tokens
    }).to_string();

    // Write body to a temp file — the base64 screenshot can be several MB,
    // which exceeds ARG_MAX if passed as a shell argument via -d.
    const BODY_PATH: &str = "/tmp/pointer_body.json";
    std::fs::write(BODY_PATH, &body).map_err(|e| format!("write body: {e}"))?;
    let out = Command::new("curl")
        .args(["-s","-X","POST", api_url,
               "-H","Content-Type: application/json",
               "-H",&format!("Authorization: Bearer {api_key}"),
               "--data-binary", &format!("@{BODY_PATH}")])
        .output().map_err(|e| format!("curl: {e}"))?;

    let raw = String::from_utf8_lossy(&out.stdout);
    // Detect non-vision model and surface a clear error immediately.
    if raw.contains("must be a string") {
        return Err(format!(
            "Model '{model}' does not support vision/images. \
             Use a vision-capable model such as meta-llama/llama-4-scout-17b-16e-instruct"
        ));
    }
    // Rate limit: sleep the suggested duration then return error so the outer
    // loop retries rather than hammering the API immediately.
    if raw.contains("rate_limit_exceeded") {
        let wait = parse_retry_after(&raw).unwrap_or(6.0);
        log_info!("rate limited — waiting {wait:.1}s");
        std::thread::sleep(std::time::Duration::from_millis((wait * 1000.0) as u64 + 500));
        return Err(format!("rate_limit — waited {wait:.1}s, retrying"));
    }
    let resp: GroqResp = serde_json::from_str(&raw)
        .map_err(|e| format!("Groq parse error: {e}\n---\n{raw}"))?;
    let content = resp.choices.into_iter().next().ok_or("Groq: no choices")?.message.content;
    log_ai!("→ {content}");

    if uitars {
        return parse_uitars_response(&content, img_w, img_h);
    }

    let json_str = content.trim()
        .trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    serde_json::from_str::<StepJson>(json_str)
        .map_err(|e| format!("step JSON parse: {e}\n---\n{content}"))
}

// ── System prompts ────────────────────────────────────────────────────────────

// UI-TARS is trained with the Thought/Action format using 0-999 normalized coords.
const UITARS_SYSTEM_PROMPT: &str = r#"You are a GUI agent. Given a screenshot and a task, output your next action.

Always reply in this exact format:
Thought: <one sentence explaining what you see and what to do next>
Action: <action call>

Available actions:
  click(point='<point>[[x,y]]</point>')            - left click
  double_click(point='<point>[[x,y]]</point>')     - double click
  right_click(point='<point>[[x,y]]</point>')      - right click
  hover(point='<point>[[x,y]]</point>')            - move cursor
  type(content='<text>')                           - type text
  scroll(point='<point>[[x,y]]</point>', direction='up|down|left|right', amount='3')
  finished(content='<answer or confirmation>')     - task is done; put full answer here

Coordinates use a 0–999 scale where (0,0) is top-left and (999,999) is bottom-right.

Rules:
- If the task is a question ("where is X", "what is Y"), answer with finished(content='...') immediately based on what is visible.
- Do NOT repeat the same action more than twice with no new outcome — try something different.
- Only one action per turn."#;

const SYSTEM_PROMPT: &str = r#"You are a macOS GUI tour-guide. Each turn you receive a screenshot and must identify the SINGLE NEXT action the user must take.

━━ COORDINATE RULES (STRICT) ━━
• Origin (0,0) = TOP-LEFT of the image. x→right, y→DOWN.
• The valid pixel ranges are stated in every user message. You MUST stay inside them.
• Return the CENTRE pixel of the visible element you are targeting.
• NEVER guess coordinates for elements not clearly visible in the screenshot.

━━ SCROLL vs CLICK RULE ━━
• If the target element is visible ANYWHERE — even partially — click it directly. Do NOT scroll past it.
• Only use Scroll when the target is completely off-screen and invisible.
• After each Scroll, look for the element before scrolling again.

━━ LOOP / STUCK RULE ━━
• If an action appears 2+ times in "Steps done so far" with no new outcome, STOP repeating it.
• Try a completely different element, or if enough information is visible to answer the goal, set is_final=true and answer directly.

━━ RESEARCH / INFORMATIONAL GOALS ━━
• If the goal is a QUESTION — "where is X", "what is Y", "which is best", "how do I", "find X", "show me X" — answer it DIRECTLY from what you can already see on screen. Set is_final=true immediately and put your complete answer in description.
• Do NOT open files, click around, or navigate to "look for more" when the answer is already derivable from the current screenshot or from your knowledge of the visible project structure.
• Example: goal "where is the agent code" → look at the visible file tree or open editor → answer "The agent code is in src/agent.rs" → is_final=true. Do NOT click agent.rs repeatedly.
• Once you can see enough to give a useful answer, stop and answer. Never loop through elements searching for "more details" when the information is already visible.

━━ HISTORY RULE ━━
• "Steps done so far" = already completed. Do NOT repeat them.
• Trust the CURRENT screenshot over your memory of what the page looked like.

━━ OUTPUT ━━
Raw JSON only — no markdown fences, no explanation:
{"x":<int>,"y":<int>,"action":"<Click|Type|Double-click|Right-click|Scroll|Hover>","description":"<what to do or final answer>","reason":"<why this is the right next step>","is_final":<true|false>}"#;
