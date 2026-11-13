//! SSE line parsing for `POST /api/chat`: bytes off the wire become [`ChatEvent`]
//! values. This module knows nothing about HTTP or Slint; it only turns lines
//! into events, so the parsing is testable without a server or a window.

use super::bridge::ChatEvent;

/// Parse one SSE line. Returns `None` for lines that carry no event: blank
/// lines, keep-alive comments (`:`), lines without a `data:` prefix, the
/// `[DONE]` terminator, and lines that are not valid `ChatEvent` JSON. One
/// malformed line never kills the turn; the caller just skips it.
pub fn parse_event_line(line: &str) -> Option<ChatEvent> {
    let line = line.trim_end();
    if line.is_empty() || line.starts_with(':') {
        return None;
    }
    let data = line.strip_prefix("data:")?.trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    serde_json::from_str(data).ok()
}

/// Accumulates bytes from the response stream and yields complete events.
/// Lines are split on `\n`; a trailing partial line is held until the rest
/// arrives. The server sends one JSON object per `data:` line and closes the
/// stream after `done`, `error`, or `cancelled`.
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    pub fn new() -> Self {
        SseParser { buf: Vec::new() }
    }

    /// Feed a chunk of bytes. Returns every event completed by this chunk.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<ChatEvent> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(pos) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            if let Some(event) = parse_event_line(&String::from_utf8_lossy(&line)) {
                events.push(event);
            }
        }
        events
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_skips_comments_blanks_and_done() {
        let mut parser = SseParser::new();
        let events = parser.feed(b": keep-alive\n\ndata: [DONE]\n");
        assert!(
            events.is_empty(),
            "comments, blanks, and [DONE] carry no event"
        );
    }

    #[test]
    fn parser_splits_lines_across_chunks() {
        let mut parser = SseParser::new();
        let first = parser.feed(b"data: {\"type\": \"tex");
        assert!(first.is_empty(), "a partial line yields nothing yet");
        let second = parser.feed(b"t\", \"text\": \"Hel\"}\n\ndata: {\"type\": \"done\", \"text\": \"Hello\", \"title\": \"Hi\"}\n");
        assert_eq!(
            second.len(),
            2,
            "the split line and the next line both parse"
        );
        assert!(matches!(second[0], ChatEvent::Text { .. }));
        assert!(matches!(second[1], ChatEvent::Done { .. }));
    }

    #[test]
    fn parser_skips_malformed_lines() {
        let mut parser = SseParser::new();
        let events =
            parser.feed(b"data: not json\n\ndata: {\"type\": \"text\", \"text\": \"ok\"}\n");
        assert_eq!(events.len(), 1, "one malformed line never kills the turn");
    }
}
