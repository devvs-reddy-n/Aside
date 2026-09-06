//! A minimal, provider-agnostic Server-Sent-Events (SSE) decoder.
//!
//! All four LLM providers stream a chat reply as `text/event-stream`, per
//! the SSE spec: newline-terminated `field: value` lines, a blank line
//! ends one event, `data:` may repeat (joined by `\n`), lines starting
//! with `:` are comments to be ignored. This is the one piece of that
//! framing shared by every provider — `openai.rs`/`groq.rs` only care
//! about `data:`, `anthropic.rs` also needs the `event:` name to tell
//! `content_block_delta` apart from `message_start`/`message_stop`.
//!
//! Mirrors `stt::SttEngine`'s "buffer internally, drain what's ready" shape:
//! callers feed in each chunk of bytes as it arrives from `reqwest`'s
//! `bytes_stream()`, in whatever order and size the network delivers them
//! (never guaranteed to land on an event boundary), and get back however
//! many complete events that chunk completed.

/// One decoded SSE event.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SseEvent {
    /// The `event:` field, if the block had one. `None` means the
    /// provider's default/unnamed event (OpenAI, Groq, and Gemini never
    /// name their events; Anthropic always does).
    pub event: Option<String>,
    /// Every `data:` line's value for this block, joined with `\n` per the
    /// SSE spec (almost always a single line in practice for these APIs).
    pub data: String,
}

/// Incremental decoder — owns only the not-yet-complete tail of the byte
/// stream between calls.
#[derive(Debug, Default)]
pub struct SseDecoder {
    buf: String,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds in the next chunk of raw response text and returns however
    /// many complete events it finished. Any trailing partial event stays
    /// buffered for the next call.
    pub fn push(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buf.push_str(chunk);
        // Normalize CRLF up front so the "\n\n" boundary check below works
        // regardless of which line ending the server used.
        if self.buf.contains('\r') {
            self.buf = self.buf.replace("\r\n", "\n");
        }

        let mut events = Vec::new();
        loop {
            let Some(boundary) = self.buf.find("\n\n") else {
                break;
            };
            let block: String = self.buf.drain(..boundary).collect();
            self.buf.drain(..2); // consume the "\n\n" itself
            if let Some(event) = parse_block(&block) {
                events.push(event);
            }
        }
        events
    }
}

/// Parses one event block (everything between two blank lines) into an
/// `SseEvent`. Returns `None` for a block with no `data:` line at all (a
/// pure comment block, or a bare `event:` with nothing to carry).
fn parse_block(block: &str) -> Option<SseEvent> {
    let mut event_name: Option<String> = None;
    let mut data_lines: Vec<&str> = Vec::new();

    for line in block.split('\n') {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event_name = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            // A single leading space after the colon is conventional and
            // stripped; anything past that is kept verbatim (JSON bodies
            // can legitimately contain further whitespace).
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
        // Other fields (id:, retry:) are part of the spec but unused by any
        // of these four APIs' chat-streaming responses — ignored, not
        // errored on, so a provider adding one later can't break parsing.
    }

    if data_lines.is_empty() {
        return None;
    }
    Some(SseEvent {
        event: event_name,
        data: data_lines.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_complete_event_in_one_push_is_decoded() {
        let mut dec = SseDecoder::new();
        let events = dec.push("data: {\"hello\":\"world\"}\n\n");
        assert_eq!(
            events,
            vec![SseEvent {
                event: None,
                data: "{\"hello\":\"world\"}".to_string()
            }]
        );
    }

    #[test]
    fn an_event_split_across_two_pushes_is_reassembled() {
        let mut dec = SseDecoder::new();
        assert!(dec.push("data: {\"partial").is_empty());
        let events = dec.push("\":true}\n\n");
        assert_eq!(
            events,
            vec![SseEvent {
                event: None,
                data: "{\"partial\":true}".to_string()
            }]
        );
    }

    #[test]
    fn a_boundary_split_exactly_at_the_double_newline_still_works() {
        let mut dec = SseDecoder::new();
        assert!(dec.push("data: x\n").is_empty());
        let events = dec.push("\n");
        assert_eq!(
            events,
            vec![SseEvent {
                event: None,
                data: "x".to_string()
            }]
        );
    }

    #[test]
    fn multiple_events_in_one_chunk_are_all_returned_in_order() {
        let mut dec = SseDecoder::new();
        let events = dec.push("data: one\n\ndata: two\n\ndata: three\n\n");
        let texts: Vec<&str> = events.iter().map(|e| e.data.as_str()).collect();
        assert_eq!(texts, vec!["one", "two", "three"]);
    }

    #[test]
    fn a_named_event_carries_its_name() {
        let mut dec = SseDecoder::new();
        let events = dec.push("event: content_block_delta\ndata: {\"text\":\"hi\"}\n\n");
        assert_eq!(events[0].event.as_deref(), Some("content_block_delta"));
        assert_eq!(events[0].data, "{\"text\":\"hi\"}");
    }

    #[test]
    fn multiline_data_fields_are_joined_with_newlines() {
        let mut dec = SseDecoder::new();
        let events = dec.push("data: line one\ndata: line two\n\n");
        assert_eq!(events[0].data, "line one\nline two");
    }

    #[test]
    fn comment_lines_are_ignored() {
        let mut dec = SseDecoder::new();
        let events = dec.push(": keep-alive\ndata: real\n\n");
        assert_eq!(
            events,
            vec![SseEvent {
                event: None,
                data: "real".to_string()
            }]
        );
    }

    #[test]
    fn a_block_with_only_an_event_name_and_no_data_produces_nothing() {
        let mut dec = SseDecoder::new();
        let events = dec.push("event: ping\n\n");
        assert!(events.is_empty());
    }

    #[test]
    fn crlf_line_endings_are_handled_the_same_as_lf() {
        let mut dec = SseDecoder::new();
        let events = dec.push("data: crlf-test\r\n\r\n");
        assert_eq!(
            events,
            vec![SseEvent {
                event: None,
                data: "crlf-test".to_string()
            }]
        );
    }

    #[test]
    fn the_done_sentinel_round_trips_as_plain_data() {
        // OpenAI/Groq send a literal "data: [DONE]\n\n" — the decoder's job
        // is just framing; recognizing [DONE] is each provider's own
        // extract_delta's job, not this module's.
        let mut dec = SseDecoder::new();
        let events = dec.push("data: [DONE]\n\n");
        assert_eq!(events[0].data, "[DONE]");
    }

    #[test]
    fn an_empty_push_produces_no_events_and_does_not_panic() {
        let mut dec = SseDecoder::new();
        assert!(dec.push("").is_empty());
    }
}
