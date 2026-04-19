use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::Deserialize;
use std::io::Read;
use std::process::Command;

pub const GROQ_MODEL_DEFAULT: &str = "meta-llama/llama-4-scout-17b-16e-instruct";
pub const GROQ_URL_DEFAULT:   &str = "https://api.groq.com/openai/v1/chat/completions";

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
    eprintln!("[pointer] screenshot {actual_w}×{actual_h} (logical {logical_w}×{logical_h})");
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
                eprintln!("[pointer] attempt {}: out-of-bounds ({:.0},{:.0}) — retrying with scroll hint",
                          attempt+1, s.x, s.y);
                extra = format!("{base_extra}{oob_hint}");
            }
            Err(e) if attempt < 2 => eprintln!("[pointer] attempt {}: {e} — retrying", attempt+1),
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

fn query_groq_once(
    goal: &str, app: &str, window: &str, url: Option<&str>,
    history: &[String], screenshot_b64: &str, img_w: f64, img_h: f64,
    api_key: &str, api_url: &str, model: &str, extra_hint: &str,
) -> Result<StepJson, String> {
    let history_text = if history.is_empty() { "None yet.".to_owned() } else {
        history.iter().enumerate().map(|(i,s)| format!("{}. {s}", i+1)).collect::<Vec<_>>().join("\n")
    };
    let url_line = url.map(|u| format!("url:    {u}\n")).unwrap_or_default();
    let user_text = format!(
        "app:    {app}\nwindow: {window}\n{url_line}goal:   {goal}\n\n\
         Screenshot size: {img_w:.0} × {img_h:.0} px  (origin TOP-LEFT, y DOWN)\n\
         Valid x range: 1 – {:.0}   Valid y range: 1 – {:.0}\n\n\
         Steps done so far:\n{history_text}\n\n\
         Identify the single next UI element and return its centre pixel.{extra_hint}",
        img_w-1.0, img_h-1.0
    );
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role":"system","content": SYSTEM_PROMPT},
            {"role":"user","content":[
                {"type":"text","text": user_text},
                {"type":"image_url","image_url":{"url": format!("data:image/png;base64,{screenshot_b64}")}}
            ]}
        ],
        "temperature": 0,
        "max_tokens": 512
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
    let resp: GroqResp = serde_json::from_str(&raw)
        .map_err(|e| format!("Groq parse error: {e}\n---\n{raw}"))?;
    let content = resp.choices.into_iter().next().ok_or("Groq: no choices")?.message.content;
    eprintln!("[pointer] AI → {content}");
    let json_str = content.trim()
        .trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    serde_json::from_str::<StepJson>(json_str)
        .map_err(|e| format!("step JSON parse: {e}\n---\n{content}"))
}

// ── System prompt ─────────────────────────────────────────────────────────────

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
• If the goal is to find, compare, or recommend information (e.g. "which X is best", "what is the price of Y"), you do NOT need to click into every row or open every detail page.
• Once you can see enough information on screen to give a useful answer, set is_final=true and put your complete answer in description (e.g. "Based on the pricing page, DALL-E 3 HD is $0.080/image — the cheapest option visible is…").
• Never loop through the same rows repeatedly looking for "more details" when the information is already visible.

━━ HISTORY RULE ━━
• "Steps done so far" = already completed. Do NOT repeat them.
• Trust the CURRENT screenshot over your memory of what the page looked like.

━━ OUTPUT ━━
Raw JSON only — no markdown fences, no explanation:
{"x":<int>,"y":<int>,"action":"<Click|Type|Double-click|Right-click|Scroll|Hover>","description":"<what to do or final answer>","reason":"<why this is the right next step>","is_final":<true|false>}"#;
