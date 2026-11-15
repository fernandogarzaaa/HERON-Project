//! SSE line parsing for `POST /api/chat`: bytes off the wire become [`ChatEvent`]
//! values. This module knows nothing about HTTP or Slint; it only turns lines
//! into events, so the parsing is testable without a server or a window.

use super::bridge::ChatEvent;
use anyhow::{bail, Result};

// One malformed or hostile SSE response must not consume unlimited memory.
// Normal chat token deltas are far smaller than this limit.
const MAX_SSE_LINE_BYTES: usize = 4 * 1024 * 1024;

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

    /// Feed bytes without buffering an unlimited newline-free frame.
    /// The caller treats a frame error as a broken stream and fails the turn
    /// rather than silently discarding data or leaving the UI busy.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<ChatEvent>> {
        let mut remaining = chunk;
        let mut events = Vec::new();
        while let Some(pos) = remaining.iter().position(|b| *b == b'\n') {
            if self.buf.len().saturating_add(pos) > MAX_SSE_LINE_BYTES {
                self.buf.clear();
                bail!("chat SSE line exceeded {MAX_SSE_LINE_BYTES} bytes");
            }
            self.buf.extend_from_slice(&remaining[..pos]);
            if let Some(event) = parse_event_line(&String::from_utf8_lossy(&self.buf)) {
                events.push(event);
            }
            self.buf.clear();
            remaining = &remaining[pos + 1..];
        }
        if self.buf.len().saturating_add(remaining.len()) > MAX_SSE_LINE_BYTES {
            self.buf.clear();
            bail!("chat SSE line exceeded {MAX_SSE_LINE_BYTES} bytes");
        }
        self.buf.extend_from_slice(remaining);
        Ok(events)
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
        let events = parser.feed(b": keep-alive\n\ndata: [DONE]\n").unwrap();
        assert!(
            events.is_empty(),
            "comments, blanks, and [DONE] carry no event"
        );
    }

    #[test]
    fn parser_splits_lines_across_chunks() {
        let mut parser = SseParser::new();
        let first = parser.feed(b"data: {\"type\": \"tex").unwrap();
        assert!(first.is_empty(), "a partial line yields nothing yet");
        let second = parser.feed(b"t\", \"text\": \"Hel\"}\n\ndata: {\"type\": \"done\", \"text\": \"Hello\", \"title\": \"Hi\"}\n").unwrap();
        assert_eq!(
            second.len(),
            2,
            "the split line and the next line both parse"
        );
        assert!(matches!(second[0], ChatEvent::Text { .. }));
        assert!(matches!(second[1], ChatEvent::Done { .. }));
    }

    #[test]
    fn parser_rejects_oversized_partial_frame_across_chunks() {
        let mut parser = SseParser::new();
        // A server that never sends a newline cannot grow memory forever.
        assert!(parser.feed(&vec![b'a'; MAX_SSE_LINE_BYTES]).unwrap().is_empty());
        assert!(parser.feed(b"a").is_err());
        // Clearing the rejected buffer permits a later valid reply.
        let events = parser.feed(b"data: {\"type\":\"cancelled\"}\n").unwrap();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn parser_accepts_large_chunks_of_small_valid_events() {
        let mut parser = SseParser::new();
        let frame = b"data: {\"type\":\"text\",\"text\":\"ok\"}\n";
        // Transport chunk boundaries are arbitrary; 1,500 valid events
        // arriving together must behave like 1,500 separate reads.
        let chunk = frame.repeat(1500);
        assert_eq!(parser.feed(&chunk).unwrap().len(), 1500);
    }

    #[test]
    fn parser_skips_malformed_lines() {
        let mut parser = SseParser::new();
        let events =
            parser.feed(b"data: not json\n\ndata: {\"type\": \"text\", \"text\": \"ok\"}\n").unwrap();
        assert_eq!(events.len(), 1, "one malformed line never kills the turn");
    }
}
