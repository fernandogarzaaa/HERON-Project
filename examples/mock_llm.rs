//! A tiny OpenAI-compatible server for offline demos and end-to-end tests.
//! It streams responses and drives real tool calls with simple keyword rules.
//!   cargo run --example mock_llm -- 8089
use axum::{routing::post, Json, Router};
use serde_json::{json, Value};

fn chunk(delta: Value) -> String {
    format!("data: {}\n\n", json!({"choices": [{"index": 0, "delta": delta}]}))
}

fn text_stream(text: &str) -> String {
    let mut out = String::new();
    for word in text.split_inclusive(' ') {
        out.push_str(&chunk(json!({"content": word})));
    }
    out
}

fn tool_stream(name: &str, args: Value) -> String {
    let a = args.to_string();
    let (first, second) = a.split_at(a.len() / 2);
    chunk(json!({"tool_calls": [{"index": 0, "id": format!("call_{}", name), "type": "function", "function": {"name": name, "arguments": first}}]}))
        + &chunk(json!({"tool_calls": [{"index": 0, "function": {"arguments": second}}]}))
}

async fn completions(Json(body): Json<Value>) -> impl axum::response::IntoResponse {
    let msgs = body["messages"].as_array().cloned().unwrap_or_default();
    let last = msgs.last().cloned().unwrap_or(json!({}));
    let last_user = msgs.iter().rev().find(|m| m["role"] == "user").and_then(|m| m["content"].as_str()).unwrap_or("").to_lowercase();
    let has_tools = body["tools"].as_array().map(|t| !t.is_empty()).unwrap_or(false);
    let mut out = if last["role"] == "tool" {
        let result = last["content"].as_str().unwrap_or("");
        text_stream(&format!("Done. Here is what I found:\n\n```\n{}\n```\n\nAnything **else** you need?", result.chars().take(400).collect::<String>()))
    } else if last_user.contains("proactive side of a personal assistant") {
        text_stream("[{\"title\":\"Write up notes from your trip plan\",\"why\":\"You changed three files in trip/ in the last hour\",\"prompt\":\"Please write a short hello note\"}]")
    } else if last_user.contains("get to know its user") {
        if last_user.contains("dinner") {
            text_stream("[{\"content\":\"Maria Lopez is close to the user and invited them to dinner on Friday at 7\",\"kind\":\"person\",\"importance\":0.7},{\"content\":\"Invoice 42 from Billing is due on 30 November\",\"kind\":\"fact\",\"importance\":0.6}]")
        } else if last_user.contains("acme") {
            text_stream("[{\"content\":\"The user is doing a rebrand for the client Acme, due 15 December\",\"kind\":\"project\",\"importance\":0.8}]")
        } else {
            text_stream("[]")
        }
    } else if last_user.contains("background awareness of a personal assistant") {
        text_stream("[{\"content\":\"The user is planning a trip and keeps the plan in the trip folder\",\"kind\":\"project\"}]")
    } else if !has_tools || last_user.contains("extract only durable facts") {
        if last_user.contains("vegetarian") { text_stream("[{\"content\":\"The user is vegetarian\",\"kind\":\"profile\",\"importance\":0.8}]") } else { text_stream("[]") }
    } else if last_user.contains("list") || last_user.contains("folder") {
        tool_stream("list_dir", json!({"path": "."}))
    } else if last_user.contains("write") {
        tool_stream("write_file", json!({"path": "notes/hello.md", "content": "# Hello\n\nWritten by Heron.\n"}))
    } else if last_user.contains("danger") {
        tool_stream("shell", json!({"command": "rm -rf build"}))
    } else if last_user.contains("run") {
        tool_stream("shell", json!({"command": "echo hello from the shell"}))
    } else if last_user.contains("remind") {
        tool_stream("schedule_add", json!({"when": "tomorrow 9:00", "prompt": "Remind the user to call the dentist", "name": "Call the dentist"}))
    } else if last_user.contains("remember that") {
        tool_stream("remember", json!({"content": last_user.replace("remember that", "The user said:"), "kind": "fact"}))
    } else {
        let sys = msgs.first().and_then(|m| m["content"].as_str()).unwrap_or("");
        let knows = if sys.contains("vegetarian") { " I remember that you are vegetarian." } else { "" };
        text_stream(&format!("Hello! I am the offline demo model.{knows} Try asking me to *list this folder*, **write a file**, or `run` a command."))
    };
    out.push_str(&format!("data: {}\n\ndata: [DONE]\n\n", json!({"choices": [], "usage": {"prompt_tokens": 120, "completion_tokens": 30}})));
    ([("content-type", "text/event-stream")], out)
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::args().nth(1).and_then(|p| p.parse().ok()).unwrap_or(8089);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    println!("mock LLM listening on http://127.0.0.1:{port}/v1");
    axum::serve(listener, Router::new().route("/v1/chat/completions", post(completions))).await.unwrap();
}
