use std::collections::VecDeque;

// ── Step memory ───────────────────────────────────────────────────────────────
//
// Two-tier design:
//   recent   — last MEM_WINDOW steps verbatim  →  O(1) bounded
//   summary  — one line per step that aged out →  O(n) but compressed ~10×
//   mistakes — unique repeated-with-no-progress actions → O(distinct actions)
//
// The AI sees full detail for recent steps, a compact summary for older ones,
// and an explicit "don't repeat" list — preventing mistake loops without
// wasting tokens on stale verbatim history.

pub const MEM_WINDOW: usize = 6;

#[derive(Debug)]
struct MemEntry {
    step_num: usize,
    action:   String,
    desc:     String,
}

#[derive(Debug)]
pub struct Memory {
    recent:   VecDeque<MemEntry>, // bounded to MEM_WINDOW
    summary:  String,             // compressed lines for aged-out steps
    mistakes: Vec<String>,        // unique actions confirmed to make no progress
    total:    usize,
}

impl Memory {
    pub fn new() -> Self {
        Self {
            recent:   VecDeque::with_capacity(MEM_WINDOW + 1),
            summary:  String::new(),
            mistakes: Vec::new(),
            total:    0,
        }
    }

    /// Record a completed step. Oldest entry is compressed into summary when window fills.
    pub fn push(&mut self, step_num: usize, action: &str, desc: &str) {
        self.total += 1;
        self.recent.push_back(MemEntry {
            step_num,
            action: action.to_owned(),
            desc:   desc.to_owned(),
        });
        if self.recent.len() > MEM_WINDOW {
            if let Some(e) = self.recent.pop_front() {
                if !self.summary.is_empty() { self.summary.push('\n'); }
                self.summary.push_str(&format!("  Step {}: {} — {}", e.step_num, e.action, e.desc));
            }
        }
    }

    /// If the last 3 recent entries repeat the same action, return that action name.
    pub fn stuck_action(&self) -> Option<&str> {
        if self.recent.len() < 3 { return None; }
        let mut it = self.recent.iter().rev();
        let a = it.next()?.action.as_str();
        let b = it.next()?.action.as_str();
        let c = it.next()?.action.as_str();
        if a == b && b == c { Some(a) } else { None }
    }

    /// Register the currently-repeated action as a known mistake to avoid.
    pub fn register_stuck(&mut self) {
        if let Some(last) = self.recent.back() {
            let entry = last.action.clone();
            if !self.mistakes.contains(&entry) {
                self.mistakes.push(entry);
            }
        }
    }

    pub fn total_steps(&self) -> usize { self.total }

    /// Returns a prompt-ready history block: compressed past + recent detail + mistake list.
    pub fn format_for_prompt(&self) -> String {
        if self.total == 0 {
            return "None yet.".to_owned();
        }
        let mut out = String::new();
        if !self.summary.is_empty() {
            out.push_str("Earlier steps (compressed):\n");
            out.push_str(&self.summary);
            out.push('\n');
        }
        if !self.recent.is_empty() {
            if !out.is_empty() { out.push('\n'); }
            out.push_str("Recent steps (full detail):\n");
            for e in &self.recent {
                out.push_str(&format!("  {}. {}: {}\n", e.step_num, e.action, e.desc));
            }
        }
        if !self.mistakes.is_empty() {
            out.push_str("\n⚠ DO NOT REPEAT — tried with no progress:\n");
            for m in &self.mistakes {
                out.push_str(&format!("  • {m}\n"));
            }
        }
        out
    }
}
