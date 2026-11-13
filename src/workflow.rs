//! Automations: fixed multi-step jobs that run as plain steps, calling the AI model only where a
//! step asks for it. A repeated job therefore costs nothing to re-run and behaves the same every time.
//!
//! An automation is a list of steps. Each step has an `id` and a `do`:
//!   tool      call one of Heron's tools (through the same permission checks as the agent)
//!   ask       one question to the model, no tools (the only step that needs judgment but not action)
//!   agent     a full agent run, for steps that need both judgment and tools
//!   if        choose between two lists of steps
//!   for_each  repeat steps over a list
//!   set       compute a value
//!   notify    tell the user something
//!   stop      end the run
//!
//! Values flow between steps through templates: `{{ steps.news.out | lines | first }}`.
//! Templates are deliberately not a programming language: dotted paths and a fixed set of filters,
//! no code evaluation of any kind. A path that does not exist is an error, never an empty string.
//!
//! The design is informed by workflow tools such as n8n (steps, branching, loops, per-step retry,
//! run history) but shares no code with them.

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub const MAX_STEPS_PER_RUN: usize = 200;
const DEFAULT_LOOP_MAX: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workflow {
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Step {
    pub id: String,
    #[serde(flatten)]
    pub kind: StepKind,
    /// Extra attempts if the step fails (tool steps only).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub retry: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum StepKind {
    Tool {
        tool: String,
        #[serde(default)]
        args: Value,
    },
    Ask {
        prompt: String,
    },
    Agent {
        prompt: String,
    },
    If {
        condition: String,
        #[serde(default)]
        then: Vec<Step>,
        #[serde(default, rename = "else")]
        otherwise: Vec<Step>,
    },
    ForEach {
        items: String,
        steps: Vec<Step>,
        #[serde(default)]
        max: Option<usize>,
    },
    Set {
        value: Value,
    },
    Notify {
        text: String,
        #[serde(default)]
        title: Option<String>,
    },
    Stop {
        #[serde(default)]
        reason: String,
    },
}

// ---------------------------------------------------------------- templates

fn lookup<'a>(ctx: &'a Value, path: &str) -> Result<&'a Value> {
    let mut cur = ctx;
    for part in path.split('.') {
        let part = part.trim();
        cur = match cur {
            Value::Object(m) => m.get(part),
            Value::Array(a) => part.parse::<usize>().ok().and_then(|i| a.get(i)),
            _ => None,
        }
        .ok_or_else(|| anyhow!("\"{path}\" does not exist (no \"{part}\")"))?;
    }
    Ok(cur)
}

fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn filter_arg(spec: &str) -> (String, Option<String>) {
    let spec = spec.trim();
    match spec.find('(') {
        Some(i) if spec.ends_with(')') => (spec[..i].trim().to_string(), Some(spec[i + 1..spec.len() - 1].trim().trim_matches(['"', '\'']).to_string())),
        _ => (spec.trim().to_string(), None),
    }
}

fn apply_filter(value: Value, spec: &str) -> Result<Value> {
    let (name, arg) = filter_arg(spec);
    Ok(match name.as_str() {
        "lines" => json!(as_text(&value).lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>()),
        "json" => serde_json::from_str(&as_text(&value)).map_err(|e| anyhow!("the value is not JSON: {e}"))?,
        "first" => match value {
            Value::Array(a) => a.into_iter().next().unwrap_or(Value::Null),
            other => other,
        },
        "last" => match value {
            Value::Array(mut a) => a.pop().unwrap_or(Value::Null),
            other => other,
        },
        "len" => json!(match &value {
            Value::Array(a) => a.len(),
            Value::Object(m) => m.len(),
            Value::Null => 0,
            other => as_text(other).chars().count(),
        }),
        "trim" => json!(as_text(&value).trim()),
        "lower" => json!(as_text(&value).to_lowercase()),
        "upper" => json!(as_text(&value).to_uppercase()),
        "join" => match value {
            Value::Array(a) => json!(a.iter().map(as_text).collect::<Vec<_>>().join(arg.as_deref().unwrap_or(", "))),
            other => other,
        },
        "limit" => {
            let n: usize = arg.as_deref().and_then(|a| a.parse().ok()).ok_or_else(|| anyhow!("limit needs a number, e.g. limit(5)"))?;
            match value {
                Value::Array(a) => Value::Array(a.into_iter().take(n).collect()),
                other => json!(as_text(&other).chars().take(n).collect::<String>()),
            }
        }
        "default" => {
            let empty = matches!(&value, Value::Null) || as_text(&value).trim().is_empty();
            if empty { json!(arg.unwrap_or_default()) } else { value }
        }
        other => bail!("unknown filter \"{other}\" (available: lines, json, first, last, len, trim, lower, upper, join, limit, default)"),
    })
}

/// Evaluate the inside of one `{{ ... }}`.
fn eval_expr(ctx: &Value, expr: &str) -> Result<Value> {
    let mut parts = expr.split('|');
    let path = parts.next().unwrap_or("").trim();
    let filters: Vec<&str> = parts.collect();
    // `default` rescues a missing path; nothing else does.
    let has_default = filters.iter().any(|f| f.trim().starts_with("default"));
    let mut value = match lookup(ctx, path) {
        Ok(v) => v.clone(),
        Err(_) if has_default => Value::Null,
        Err(e) => return Err(e),
    };
    for f in filters {
        value = apply_filter(value, f)?;
    }
    Ok(value)
}

/// Render a template. A string that is exactly one `{{ ... }}` keeps its type (a list stays a list);
/// otherwise values are inserted as text.
pub fn render(ctx: &Value, template: &str) -> Result<Value> {
    let t = template.trim();
    if t.starts_with("{{") && t.ends_with("}}") && t.matches("{{").count() == 1 {
        return eval_expr(ctx, &t[2..t.len() - 2]);
    }
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let end = rest[start..].find("}}").ok_or_else(|| anyhow!("a \"{{{{\" is never closed in: {template}"))?;
        out.push_str(&as_text(&eval_expr(ctx, &rest[start + 2..start + end])?));
        rest = &rest[start + end + 2..];
    }
    out.push_str(rest);
    Ok(Value::String(out))
}

/// Render every string inside a JSON value.
pub fn render_value(ctx: &Value, v: &Value) -> Result<Value> {
    Ok(match v {
        Value::String(s) => render(ctx, s)?,
        Value::Array(a) => Value::Array(a.iter().map(|x| render_value(ctx, x)).collect::<Result<_>>()?),
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| Ok((k.clone(), render_value(ctx, x)?))).collect::<Result<Map<_, _>>>()?),
        other => other.clone(),
    })
}

// ---------------------------------------------------------------- conditions

/// Find `needle` at the top level of `s` (outside `{{ }}` and quotes).
fn find_top_level(s: &str, needle: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let (mut depth, mut quote, mut i) = (0usize, None::<u8>, 0usize);
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
        } else if s[i..].starts_with("{{") {
            depth += 1;
            i += 1;
        } else if s[i..].starts_with("}}") {
            depth = depth.saturating_sub(1);
            i += 1;
        } else if depth == 0 && (b == b'"' || b == b'\'') {
            quote = Some(b);
        } else if depth == 0 && s[i..].starts_with(needle) {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn operand(ctx: &Value, raw: &str) -> Result<Value> {
    let t = raw.trim();
    if (t.starts_with('"') && t.ends_with('"') && t.len() >= 2) || (t.starts_with('\'') && t.ends_with('\'') && t.len() >= 2) {
        return Ok(json!(t[1..t.len() - 1]));
    }
    if let Ok(n) = t.parse::<f64>() {
        return Ok(json!(n));
    }
    match t {
        "true" => return Ok(json!(true)),
        "false" => return Ok(json!(false)),
        _ => {}
    }
    render(ctx, t)
}

fn is_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Array(a) => a.is_empty(),
        Value::Object(m) => m.is_empty(),
        Value::Bool(b) => !b,
        other => as_text(other).trim().is_empty(),
    }
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Evaluate a condition such as `{{ steps.mail.out | len }} > 0`, `{{ steps.page.out }} contains "sold out"`,
/// `{{ steps.x.out }} is not empty`, joined with ` and ` / ` or `.
pub fn condition(ctx: &Value, cond: &str) -> Result<bool> {
    if let Some(i) = find_top_level(cond, " or ") {
        return Ok(condition(ctx, &cond[..i])? || condition(ctx, &cond[i + 4..])?);
    }
    if let Some(i) = find_top_level(cond, " and ") {
        return Ok(condition(ctx, &cond[..i])? && condition(ctx, &cond[i + 5..])?);
    }
    let c = cond.trim();
    for (suffix, want_empty) in [(" is not empty", false), (" is empty", true)] {
        if let Some(i) = find_top_level(c, suffix).filter(|i| i + suffix.len() == c.len()) {
            return Ok(is_empty(&operand(ctx, &c[..i])?) == want_empty);
        }
    }
    for op in [" not contains ", " contains ", " == ", " != ", " >= ", " <= ", " > ", " < "] {
        let Some(i) = find_top_level(c, op) else { continue };
        let (a, b) = (operand(ctx, &c[..i])?, operand(ctx, &c[i + op.len()..])?);
        let text = |v: &Value| as_text(v).trim().to_lowercase();
        return Ok(match op.trim() {
            "contains" | "not contains" => {
                let has = match &a {
                    Value::Array(items) => items.iter().any(|x| text(x) == text(&b)),
                    other => text(other).contains(&text(&b)),
                };
                has == (op.trim() == "contains")
            }
            "==" => number(&a).zip(number(&b)).map(|(x, y)| x == y).unwrap_or_else(|| text(&a) == text(&b)),
            "!=" => number(&a).zip(number(&b)).map(|(x, y)| x != y).unwrap_or_else(|| text(&a) != text(&b)),
            cmp => {
                let (x, y) = number(&a).zip(number(&b)).ok_or_else(|| anyhow!("\"{c}\" compares things that are not numbers"))?;
                match cmp {
                    ">" => x > y,
                    "<" => x < y,
                    ">=" => x >= y,
                    _ => x <= y,
                }
            }
        });
    }
    // A bare value: true when it is not empty.
    Ok(!is_empty(&operand(ctx, c)?))
}

// ---------------------------------------------------------------- validation and description

fn references(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = text;
    while let Some(i) = rest.find("steps.") {
        let id: String = rest[i + 6..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if !id.is_empty() {
            out.push(id);
        }
        rest = &rest[i + 6..];
    }
    out
}

fn collect_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
        Value::Object(m) => m.values().for_each(|x| collect_strings(x, out)),
        _ => {}
    }
}

impl Workflow {
    pub fn parse(v: &Value) -> Result<Workflow> {
        let steps = if v.is_array() { v.clone() } else { v.get("steps").cloned().unwrap_or(Value::Null) };
        let steps: Vec<Step> = serde_json::from_value(steps).map_err(|e| anyhow!("the steps are not valid: {e}"))?;
        Ok(Workflow { steps })
    }

    /// Check an automation before it is saved: known tools, unique ids, and templates that only refer to
    /// steps that have already run by then.
    pub fn validate(&self, tools: &[String]) -> Result<()> {
        if self.steps.is_empty() {
            bail!("an automation needs at least one step");
        }
        fn walk(steps: &[Step], tools: &[String], known: &mut HashSet<String>, count: &mut usize) -> Result<()> {
            for step in steps {
                *count += 1;
                if step.id.is_empty() || !step.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    bail!("step ids use letters, digits and underscores only (got \"{}\")", step.id);
                }
                let mut texts = vec![];
                match &step.kind {
                    StepKind::Tool { tool, args } => {
                        if !tools.contains(tool) && !tool.starts_with("mcp__") {
                            bail!("step \"{}\" uses a tool that does not exist: {tool}", step.id);
                        }
                        if tool == "save_automation" || tool == "update_persona" || tool == "save_skill" {
                            bail!("step \"{}\": an automation cannot change Heron's own skills, character or automations", step.id);
                        }
                        collect_strings(args, &mut texts);
                    }
                    StepKind::Ask { prompt } | StepKind::Agent { prompt } => texts.push(prompt.clone()),
                    StepKind::If { condition, .. } => texts.push(condition.clone()),
                    StepKind::ForEach { items, .. } => texts.push(items.clone()),
                    StepKind::Set { value } => collect_strings(value, &mut texts),
                    StepKind::Notify { text, title } => {
                        texts.push(text.clone());
                        texts.extend(title.clone());
                    }
                    StepKind::Stop { reason } => texts.push(reason.clone()),
                }
                for t in &texts {
                    for id in references(t) {
                        if !known.contains(&id) {
                            bail!("step \"{}\" refers to steps.{id}, which has not run by then", step.id);
                        }
                    }
                }
                match &step.kind {
                    StepKind::If { then, otherwise, .. } => {
                        // Steps inside a branch may not have run, so later steps cannot rely on them.
                        let mut inner = known.clone();
                        walk(then, tools, &mut inner, count)?;
                        let mut inner = known.clone();
                        walk(otherwise, tools, &mut inner, count)?;
                    }
                    StepKind::ForEach { steps, .. } => {
                        let mut inner = known.clone();
                        walk(steps, tools, &mut inner, count)?;
                    }
                    _ => {}
                }
                if !known.insert(step.id.clone()) {
                    bail!("two steps are both called \"{}\"", step.id);
                }
            }
            Ok(())
        }
        let mut count = 0;
        walk(&self.steps, tools, &mut HashSet::new(), &mut count)?;
        if count > 60 {
            bail!("that is {count} steps; keep an automation under 60");
        }
        Ok(())
    }

    /// Does any step call the model? (Shown to the user: such automations spend tokens.)
    pub fn uses_model(&self) -> bool {
        fn any(steps: &[Step]) -> bool {
            steps.iter().any(|s| match &s.kind {
                StepKind::Ask { .. } | StepKind::Agent { .. } => true,
                StepKind::If { then, otherwise, .. } => any(then) || any(otherwise),
                StepKind::ForEach { steps, .. } => any(steps),
                _ => false,
            })
        }
        any(&self.steps)
    }

    /// A plain-language outline, shown to the user before they approve saving it.
    pub fn describe(&self) -> String {
        fn lines(steps: &[Step], indent: usize, out: &mut Vec<String>) {
            let pad = "   ".repeat(indent);
            for (i, s) in steps.iter().enumerate() {
                let n = format!("{pad}{}.", i + 1);
                match &s.kind {
                    StepKind::Tool { tool, args } => out.push(format!("{n} Use {tool} {}", crate::util::preview(&args.to_string(), 140))),
                    StepKind::Ask { prompt } => out.push(format!("{n} Ask the AI model: {}", crate::util::preview(prompt, 140))),
                    StepKind::Agent { prompt } => out.push(format!("{n} Let Heron work on: {}", crate::util::preview(prompt, 140))),
                    StepKind::If { condition, then, otherwise } => {
                        out.push(format!("{n} If {condition}:"));
                        lines(then, indent + 1, out);
                        if !otherwise.is_empty() {
                            out.push(format!("{pad}   Otherwise:"));
                            lines(otherwise, indent + 1, out);
                        }
                    }
                    StepKind::ForEach { items, steps, max } => {
                        out.push(format!("{n} For each of {items} (at most {}):", max.unwrap_or(DEFAULT_LOOP_MAX)));
                        lines(steps, indent + 1, out);
                    }
                    StepKind::Set { value } => out.push(format!("{n} Work out {}", crate::util::preview(&value.to_string(), 120))),
                    StepKind::Notify { text, .. } => out.push(format!("{n} Tell you: {}", crate::util::preview(text, 140))),
                    StepKind::Stop { reason } => out.push(format!("{n} Stop{}", if reason.is_empty() { String::new() } else { format!(" ({reason})") })),
                }
            }
        }
        let mut out = vec![];
        lines(&self.steps, 0, &mut out);
        out.join("\n")
    }
}

// ---------------------------------------------------------------- running

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepRecord {
    pub id: String,
    pub what: String,
    /// ok | failed | skipped | waiting
    pub status: String,
    pub output: String,
    pub ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunRecord {
    /// ok | failed | waiting | stopped
    pub status: String,
    pub steps: Vec<StepRecord>,
    /// (title, text) messages for the user.
    pub notices: Vec<(Option<String>, String)>,
    pub error: Option<String>,
}

/// What an automation can do in the outside world. Implemented by the app (real tools and model)
/// and by tests (scripted).
#[async_trait::async_trait]
pub trait Host: Send + Sync {
    /// Call a tool. Ok(text) on success; Err with the tool's message on failure or refusal.
    async fn tool(&self, name: &str, args: &Value) -> std::result::Result<String, String>;
    /// One model call without tools.
    async fn ask(&self, prompt: &str) -> Result<String>;
    /// A full agent run.
    async fn agent(&self, prompt: &str) -> Result<String>;
}

struct Run<'a> {
    host: &'a dyn Host,
    ctx: Value,
    record: RunRecord,
    executed: usize,
}

enum Flow {
    Continue,
    Stop,
}

impl Run<'_> {
    fn set_out(&mut self, id: &str, out: Value) {
        self.ctx["steps"][id] = json!({ "out": out });
    }

    fn log(&mut self, id: &str, what: &str, status: &str, output: &str, started: std::time::Instant) {
        self.record.steps.push(StepRecord { id: id.to_string(), what: what.to_string(), status: status.to_string(), output: crate::util::preview(output, 400), ms: started.elapsed().as_millis() as u64 });
    }

    fn steps<'s>(&'s mut self, steps: &'s [Step]) -> futures_util::future::BoxFuture<'s, Result<Flow>> {
        Box::pin(async move {
            for step in steps {
                self.executed += 1;
                if self.executed > MAX_STEPS_PER_RUN {
                    bail!("stopped after {MAX_STEPS_PER_RUN} steps: the automation is looping too much");
                }
                let started = std::time::Instant::now();
                match &step.kind {
                    StepKind::Tool { tool, args } => {
                        let args = render_value(&self.ctx, args).map_err(|e| anyhow!("step \"{}\": {e}", step.id))?;
                        let mut attempt = 0;
                        let out = loop {
                            match self.host.tool(tool, &args).await {
                                Ok(text) => break text,
                                // A refusal is a decision, not a glitch: never retried.
                                Err(msg) if msg.starts_with("Permission denied") => {
                                    let waiting = msg.contains("saved for the user to approve");
                                    self.log(&step.id, tool, if waiting { "waiting" } else { "failed" }, &msg, started);
                                    self.record.status = if waiting { "waiting" } else { "failed" }.into();
                                    bail!("step \"{}\" ({tool}) {}", step.id, if waiting { "needs your approval" } else { "was not allowed" });
                                }
                                Err(msg) if attempt < step.retry => {
                                    attempt += 1;
                                    let _ = msg;
                                    tokio::time::sleep(std::time::Duration::from_millis(400 * attempt as u64)).await;
                                }
                                Err(msg) => {
                                    self.log(&step.id, tool, "failed", &msg, started);
                                    bail!("step \"{}\" ({tool}) failed: {}", step.id, crate::util::preview(&msg, 200));
                                }
                            }
                        };
                        self.log(&step.id, tool, "ok", &out, started);
                        self.set_out(&step.id, json!(out));
                    }
                    StepKind::Ask { prompt } | StepKind::Agent { prompt } => {
                        let is_agent = matches!(step.kind, StepKind::Agent { .. });
                        let text = as_text(&render(&self.ctx, prompt).map_err(|e| anyhow!("step \"{}\": {e}", step.id))?);
                        let what = if is_agent { "agent" } else { "ask" };
                        let out = if is_agent { self.host.agent(&text).await } else { self.host.ask(&text).await };
                        match out {
                            Ok(out) => {
                                self.log(&step.id, what, "ok", &out, started);
                                self.set_out(&step.id, json!(out));
                            }
                            Err(e) => {
                                self.log(&step.id, what, "failed", &format!("{e:#}"), started);
                                bail!("step \"{}\" could not reach the AI model: {e:#}", step.id);
                            }
                        }
                    }
                    StepKind::If { condition: cond, then, otherwise } => {
                        let yes = condition(&self.ctx, cond).map_err(|e| anyhow!("step \"{}\": {e}", step.id))?;
                        self.log(&step.id, "if", "ok", if yes { "yes" } else { "no" }, started);
                        self.set_out(&step.id, json!(yes));
                        if let Flow::Stop = self.steps(if yes { then } else { otherwise }).await? {
                            return Ok(Flow::Stop);
                        }
                    }
                    StepKind::ForEach { items, steps, max } => {
                        let list = match render(&self.ctx, items).map_err(|e| anyhow!("step \"{}\": {e}", step.id))? {
                            Value::Array(a) => a,
                            Value::Null => vec![],
                            other => as_text(&other).lines().map(str::trim).filter(|l| !l.is_empty()).map(|l| json!(l)).collect(),
                        };
                        let cap = max.unwrap_or(DEFAULT_LOOP_MAX).min(DEFAULT_LOOP_MAX * 4);
                        self.log(&step.id, "for_each", "ok", &format!("{} item(s){}", list.len(), if list.len() > cap { format!(", first {cap} used") } else { String::new() }), started);
                        self.set_out(&step.id, json!(list.len().min(cap)));
                        for (index, item) in list.into_iter().take(cap).enumerate() {
                            self.ctx["item"] = item;
                            self.ctx["index"] = json!(index);
                            if let Flow::Stop = self.steps(steps).await? {
                                return Ok(Flow::Stop);
                            }
                        }
                        self.ctx["item"] = Value::Null;
                    }
                    StepKind::Set { value } => {
                        let v = render_value(&self.ctx, value).map_err(|e| anyhow!("step \"{}\": {e}", step.id))?;
                        self.log(&step.id, "set", "ok", &as_text(&v), started);
                        self.set_out(&step.id, v);
                    }
                    StepKind::Notify { text, title } => {
                        let body = as_text(&render(&self.ctx, text).map_err(|e| anyhow!("step \"{}\": {e}", step.id))?);
                        let title = match title {
                            Some(t) => Some(as_text(&render(&self.ctx, t).map_err(|e| anyhow!("step \"{}\": {e}", step.id))?)),
                            None => None,
                        };
                        self.log(&step.id, "notify", "ok", &body, started);
                        self.set_out(&step.id, json!(body.clone()));
                        self.record.notices.push((title, body));
                    }
                    StepKind::Stop { reason } => {
                        self.log(&step.id, "stop", "ok", reason, started);
                        self.record.status = "stopped".into();
                        return Ok(Flow::Stop);
                    }
                }
            }
            Ok(Flow::Continue)
        })
    }
}

/// Run an automation. `event` is what set it off (for triggers), available as `{{ trigger.event }}`.
pub async fn run(host: &dyn Host, workflow: &Workflow, event: Option<&str>) -> RunRecord {
    let now = chrono::Local::now();
    let ctx = json!({
        "steps": {},
        "trigger": {"event": event.unwrap_or("")},
        "now": now.format("%Y-%m-%d %H:%M").to_string(),
        "today": now.format("%Y-%m-%d").to_string(),
        "weekday": now.format("%A").to_string(),
        "item": null,
        "index": 0,
    });
    let mut run = Run { host, ctx, record: RunRecord { status: "ok".into(), steps: vec![], notices: vec![], error: None }, executed: 0 };
    if let Err(e) = run.steps(&workflow.steps).await {
        if run.record.status == "ok" {
            run.record.status = "failed".into();
        }
        run.record.error = Some(format!("{e:#}"));
    }
    run.record
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        fail_first: Mutex<u32>,
    }

    #[async_trait::async_trait]
    impl Host for Fake {
        async fn tool(&self, name: &str, args: &Value) -> std::result::Result<String, String> {
            self.calls.lock().unwrap().push(format!("{name} {args}"));
            match name {
                "web_fetch" => Ok("Concert tickets\nStatus: SOLD OUT\nNext release: Friday".into()),
                "list_dir" => Ok("a.md\nb.md\nc.md".into()),
                "flaky" => {
                    let mut left = self.fail_first.lock().unwrap();
                    if *left > 0 {
                        *left -= 1;
                        Err("Error: temporary network problem".into())
                    } else {
                        Ok("worked".into())
                    }
                }
                "write_file" => Err("Permission denied: nobody is here to approve this right now, so it was NOT done. It has been saved for the user to approve later".into()),
                "shell" => Err("Permission denied: the user has switched this ability off in Settings".into()),
                other => Ok(format!("ran {other}")),
            }
        }
        async fn ask(&self, prompt: &str) -> Result<String> {
            self.calls.lock().unwrap().push(format!("ask {prompt}"));
            Ok("A short summary.".into())
        }
        async fn agent(&self, prompt: &str) -> Result<String> {
            self.calls.lock().unwrap().push(format!("agent {prompt}"));
            Ok("Agent did it.".into())
        }
    }

    fn wf(v: Value) -> Workflow {
        Workflow::parse(&v).unwrap()
    }

    #[test]
    fn templates_and_filters() {
        let ctx = json!({"steps": {"a": {"out": "one\n two \n\nthree"}, "n": {"out": 3}, "j": {"out": "{\"items\":[{\"name\":\"x\"}]}"}}, "today": "2026-11-12", "item": "f.md"});
        assert_eq!(render(&ctx, "{{ steps.a.out | lines }}").unwrap(), json!(["one", "two", "three"]));
        assert_eq!(render(&ctx, "{{ steps.a.out | lines | first | upper }}").unwrap(), json!("ONE"));
        assert_eq!(render(&ctx, "{{ steps.a.out | lines | len }}").unwrap(), json!(3));
        assert_eq!(render(&ctx, "{{ steps.a.out | lines | limit(2) | join(\" + \") }}").unwrap(), json!("one + two"));
        assert_eq!(render(&ctx, "Report for {{today}}: {{ steps.n.out }} new ({{ item }})").unwrap(), json!("Report for 2026-11-12: 3 new (f.md)"));
        assert_eq!(render(&ctx, "{{ steps.j.out | json }}").unwrap()["items"][0]["name"], "x");
        assert_eq!(render(&ctx, "{{ steps.missing.out | default(\"none\") }}").unwrap(), json!("none"));
        // a missing value is an error, never a silent blank
        assert!(render(&ctx, "{{ steps.missing.out }}").unwrap_err().to_string().contains("does not exist"));
        assert!(render(&ctx, "{{ steps.a.out | explode }}").unwrap_err().to_string().contains("unknown filter"));
        assert!(render(&ctx, "{{ steps.a.out").is_err());
        // no code evaluation: this is just a path that does not exist
        assert!(render(&ctx, "{{ process.exit(1) }}").is_err());
    }

    #[test]
    fn conditions() {
        let ctx = json!({"steps": {"page": {"out": "Status: SOLD OUT or maybe not"}, "n": {"out": "12"}, "list": {"out": ["a", "b"]}, "blank": {"out": "  "}}});
        assert!(condition(&ctx, "{{ steps.page.out }} contains \"sold out\"").unwrap());
        assert!(condition(&ctx, "{{ steps.page.out }} not contains \"available\"").unwrap());
        assert!(condition(&ctx, "{{ steps.n.out }} > 10").unwrap());
        assert!(!condition(&ctx, "{{ steps.n.out }} <= 10").unwrap());
        assert!(condition(&ctx, "{{ steps.list.out | len }} == 2").unwrap());
        assert!(condition(&ctx, "{{ steps.list.out }} contains \"b\"").unwrap());
        assert!(condition(&ctx, "{{ steps.blank.out }} is empty").unwrap());
        assert!(condition(&ctx, "{{ steps.page.out }} is not empty and {{ steps.n.out }} == 12").unwrap());
        assert!(condition(&ctx, "{{ steps.n.out }} == 1 or {{ steps.n.out }} == 12").unwrap());
        // the word "or" inside the value or inside quotes does not split the condition
        assert!(condition(&ctx, "{{ steps.page.out }} contains \"OUT or maybe\"").unwrap());
        assert!(condition(&ctx, "{{ steps.page.out }} > 3").is_err());
    }

    #[tokio::test]
    async fn runs_steps_branches_and_loops_without_the_model() {
        let host = Fake::default();
        let w = wf(json!([
            {"id": "page", "do": "tool", "tool": "web_fetch", "args": {"url": "https://example.com/tickets"}},
            {"id": "check", "do": "if", "condition": "{{ steps.page.out }} contains \"sold out\"",
             "then": [{"id": "tell", "do": "notify", "title": "Tickets", "text": "Still sold out. {{ steps.page.out | lines | last }}"}],
             "else": [{"id": "buy", "do": "notify", "text": "Tickets are available!"}]},
            {"id": "files", "do": "tool", "tool": "list_dir", "args": {"path": "."}},
            {"id": "each", "do": "for_each", "items": "{{ steps.files.out | lines }}", "max": 2,
             "steps": [{"id": "note", "do": "tool", "tool": "task_add", "args": {"title": "Review {{ item }} (#{{ index }})"}}]},
        ]));
        let r = run(&host, &w, None).await;
        assert_eq!(r.status, "ok", "{:?}", r.error);
        assert_eq!(r.notices, vec![(Some("Tickets".to_string()), "Still sold out. Next release: Friday".to_string())]);
        let calls = host.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 4, "{calls:?}");
        assert!(calls[2].contains("Review a.md (#0)") && calls[3].contains("Review b.md (#1)"));
        assert!(!calls.iter().any(|c| c.starts_with("ask") || c.starts_with("agent")), "no model call was needed");
        assert_eq!(r.steps.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), vec!["page", "check", "tell", "files", "each", "note", "note"]);
        assert!(!w.uses_model());
    }

    #[tokio::test]
    async fn model_steps_retries_refusals_and_stop() {
        // ask and agent steps receive rendered prompts
        let host = Fake::default();
        let w = wf(json!([
            {"id": "page", "do": "tool", "tool": "web_fetch", "args": {"url": "https://example.com"}},
            {"id": "sum", "do": "ask", "prompt": "Summarize: {{ steps.page.out | lines | first }}"},
            {"id": "act", "do": "agent", "prompt": "File this: {{ steps.sum.out }}"},
            {"id": "end", "do": "stop", "reason": "done"},
            {"id": "never", "do": "notify", "text": "unreachable"},
        ]));
        assert!(w.uses_model());
        let r = run(&host, &w, None).await;
        assert_eq!(r.status, "stopped");
        assert!(r.notices.is_empty());
        let calls = host.calls.lock().unwrap().clone();
        assert_eq!(calls[1], "ask Summarize: Concert tickets");
        assert_eq!(calls[2], "agent File this: A short summary.");

        // a transient failure is retried when the step asks for it...
        let host = Fake { fail_first: Mutex::new(2), ..Default::default() };
        let r = run(&host, &wf(json!([{"id": "f", "do": "tool", "tool": "flaky", "retry": 2}, {"id": "ok", "do": "notify", "text": "{{ steps.f.out }}"}])), None).await;
        assert_eq!((r.status.as_str(), r.notices[0].1.as_str()), ("ok", "worked"));
        // ...and fails the run when it does not
        let host = Fake { fail_first: Mutex::new(1), ..Default::default() };
        let r = run(&host, &wf(json!([{"id": "f", "do": "tool", "tool": "flaky"}, {"id": "n", "do": "notify", "text": "x"}])), None).await;
        assert_eq!(r.status, "failed");
        assert!(r.error.unwrap().contains("temporary network problem"));
        assert!(r.notices.is_empty(), "nothing after a failed step runs");

        // a step that needs approval parks the run; a refusal fails it; neither is retried
        let host = Fake::default();
        let r = run(&host, &wf(json!([{"id": "w", "do": "tool", "tool": "write_file", "args": {"path": "a"}, "retry": 3}])), None).await;
        assert_eq!(r.status, "waiting");
        assert_eq!(host.calls.lock().unwrap().len(), 1);
        let r = run(&Fake::default(), &wf(json!([{"id": "s", "do": "tool", "tool": "shell", "args": {"command": "ls"}}])), None).await;
        assert_eq!(r.status, "failed");

        // the trigger's event is available, and runaway loops are cut off
        let host = Fake::default();
        let r = run(&host, &wf(json!([{"id": "n", "do": "notify", "text": "Got: {{ trigger.event }}"}])), Some("These files changed: a.md")).await;
        assert_eq!(r.notices[0].1, "Got: These files changed: a.md");
        let many: Vec<Value> = (0..150).map(|i| json!(i)).collect();
        let big = Workflow { steps: vec![Step { id: "l".into(), retry: 0, kind: StepKind::ForEach { items: "{{ steps.s.out }}".into(), max: Some(500), steps: vec![Step { id: "a".into(), retry: 0, kind: StepKind::Set { value: json!("{{ item }}") } }, Step { id: "b".into(), retry: 0, kind: StepKind::Set { value: json!(1) } }] } }] };
        let seeded = Workflow { steps: [vec![Step { id: "s".into(), retry: 0, kind: StepKind::Set { value: Value::Array(many) } }], big.steps].concat() };
        let r = run(&host, &seeded, None).await;
        assert_eq!(r.status, "failed");
        assert!(r.error.unwrap().contains("looping too much"));
    }

    #[test]
    fn validation_catches_mistakes_before_saving() {
        let tools: Vec<String> = ["web_fetch", "list_dir", "task_add", "save_skill"].iter().map(|s| s.to_string()).collect();
        let ok = wf(json!([
            {"id": "page", "do": "tool", "tool": "web_fetch", "args": {"url": "https://example.com"}},
            {"id": "c", "do": "if", "condition": "{{ steps.page.out }} contains \"x\"", "then": [{"id": "t", "do": "notify", "text": "{{ steps.page.out }}"}]},
        ]));
        ok.validate(&tools).unwrap();
        assert!(ok.describe().contains("1. Use web_fetch") && ok.describe().contains("2. If") && ok.describe().contains("   1. Tell you"));
        let bad = |v: Value| wf(v).validate(&tools).unwrap_err().to_string();
        assert!(bad(json!([])).contains("at least one step"));
        assert!(bad(json!([{"id": "a", "do": "tool", "tool": "teleport"}])).contains("does not exist"));
        assert!(bad(json!([{"id": "a", "do": "notify", "text": "{{ steps.later.out }}"}, {"id": "later", "do": "set", "value": 1}])).contains("has not run by then"));
        assert!(bad(json!([{"id": "a", "do": "set", "value": 1}, {"id": "a", "do": "set", "value": 2}])).contains("both called"));
        assert!(bad(json!([{"id": "bad id", "do": "set", "value": 1}])).contains("letters, digits"));
        assert!(bad(json!([{"id": "a", "do": "tool", "tool": "save_skill", "args": {}}])).contains("cannot change Heron's own"));
        // a step inside a branch may not have run, so later steps cannot rely on it
        assert!(bad(json!([
            {"id": "c", "do": "if", "condition": "true", "then": [{"id": "inner", "do": "set", "value": 1}]},
            {"id": "n", "do": "notify", "text": "{{ steps.inner.out }}"},
        ]))
        .contains("has not run by then"));
        assert!(Workflow::parse(&json!([{"id": "a", "do": "dance"}])).is_err());
    }
}
