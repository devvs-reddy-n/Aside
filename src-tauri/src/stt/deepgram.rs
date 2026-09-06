//! Deepgram's streaming WebSocket protocol — connection shape and message
//! parsing only.
//!
//! **Deliberately not a full `SttEngine` implementation yet.** Everything in
//! this module is pure, deterministic, and unit-testable without a live
//! connection: building the connect URL, the (non-standard — `Token`, not
//! `Bearer`) auth header, and parsing the server's JSON message shapes.
//! Wiring an actual open socket with a send/receive loop into `SttEngine`
//! needs a real API key to verify against a live server at all, and doing
//! that without ever having run it against the real service would be an
//! unverified implementation presented as done — against this project's own
//! quality bar. See `docs/evidence.md`'s Phase 4 entry.
//!
//! Protocol details from the Phase 4 research, cross-checked against
//! `references/Repo1-Natively/electron/audio/DeepgramStreamingSTT.ts`'s
//! shipped client:
//! - Audio is sent as **raw binary WebSocket frames** matching the declared
//!   `encoding`/`sample_rate` — no JSON envelope around the audio itself.
//! - `Results.is_final` and the separate `UtteranceEnd` message are not the
//!   same signal: a `Results` message can be `is_final: true` mid-utterance
//!   (per Deepgram's own endpointing), so a real client accumulates
//!   `is_final` results and only treats them as one committed utterance at
//!   `UtteranceEnd`.

use serde::Deserialize;

/// What a connection needs to know before it opens.
pub struct DeepgramConfig {
    pub model: String,
    pub language: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// Dormant engine still accepts `keyterm=` from the session glossary.
    pub keyterms: Vec<String>,
}

impl Default for DeepgramConfig {
    fn default() -> Self {
        Self {
            model: "nova-3".to_string(),
            language: "en".to_string(),
            sample_rate: 16_000,
            channels: 1,
            keyterms: Vec::new(),
        }
    }
}

/// Builds the `wss://` connect URL with query parameters. `endpointing` (how
/// long a pause before Deepgram marks a `Results` message final) and
/// `utterance_end_ms` (how long before it emits a standalone `UtteranceEnd`)
/// are fixed at Natively's tuned values (300ms / 1000ms) rather than
/// exposed as config — nothing in this project's scope needs them
/// adjustable yet.
pub fn build_url(config: &DeepgramConfig) -> String {
    let mut url = format!(
        "wss://api.deepgram.com/v1/listen?model={}&language={}&encoding=linear16&\
         sample_rate={}&channels={}&interim_results=true&endpointing=300&\
         utterance_end_ms=1000&vad_events=true&smart_format=true",
        config.model, config.language, config.sample_rate, config.channels
    );
    for term in config
        .keyterms
        .iter()
        .filter(|t| !t.trim().is_empty())
        .take(40)
    {
        let encoded = term.trim().replace(' ', "%20");
        url.push_str("&keyterm=");
        url.push_str(&encoded);
    }
    url
}

/// Deepgram's auth scheme is `Authorization: Token <key>` — not `Bearer`.
/// Easy to get wrong by copying a typical REST `Bearer` header, which is
/// exactly why it's centralized here as its own function rather than
/// inlined at a future call site.
pub fn auth_header_value(api_key: &str) -> String {
    format!("Token {api_key}")
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Alternative {
    pub transcript: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Channel {
    pub alternatives: Vec<Alternative>,
}

/// Server-to-client message, discriminated by the JSON `"type"` field.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum DeepgramMessage {
    Results { is_final: bool, channel: Channel },
    SpeechStarted,
    UtteranceEnd,
    Metadata,
}

/// Parses one server message. Unrecognized `"type"` values (Deepgram may add
/// new message types) surface as a readable error rather than a panic — a
/// real client would log and ignore rather than crash the connection over
/// an unknown message.
pub fn parse_message(json: &str) -> Result<DeepgramMessage, String> {
    serde_json::from_str(json)
        .map_err(|e| format!("could not parse deepgram message: {e} (body: {json})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_expected_query_string() {
        let url = build_url(&DeepgramConfig::default());
        assert!(url.starts_with("wss://api.deepgram.com/v1/listen?"));
        assert!(url.contains("model=nova-3"));
        assert!(url.contains("language=en"));
        assert!(url.contains("encoding=linear16"));
        assert!(url.contains("sample_rate=16000"));
        assert!(url.contains("channels=1"));
        assert!(url.contains("interim_results=true"));
        assert!(url.contains("endpointing=300"));
        assert!(url.contains("utterance_end_ms=1000"));
        assert!(url.contains("vad_events=true"));
        assert!(
            !url.contains("keyterm="),
            "empty glossary must not send keyterm="
        );
    }

    #[test]
    fn keyterms_from_the_glossary_are_appended() {
        let cfg = DeepgramConfig {
            keyterms: vec!["STEM OPT".into(), "Redshift".into()],
            ..DeepgramConfig::default()
        };
        let url = build_url(&cfg);
        assert!(url.contains("keyterm=STEM%20OPT"), "got {url}");
        assert!(url.contains("keyterm=Redshift"), "got {url}");
    }

    #[test]
    fn auth_header_uses_token_not_bearer() {
        // Deepgram's own scheme — not Bearer. Getting this wrong would
        // fail auth silently confusingly (a 401 that looks like a bad key
        // when it's actually the wrong scheme).
        assert_eq!(auth_header_value("dg_abc123"), "Token dg_abc123");
    }

    #[test]
    fn parses_an_interim_results_message() {
        let json = r#"{"type":"Results","is_final":false,"channel":{"alternatives":[{"transcript":"so tell me","confidence":0.87}]}}"#;
        let msg = parse_message(json).unwrap();
        match msg {
            DeepgramMessage::Results { is_final, channel } => {
                assert!(!is_final);
                assert_eq!(channel.alternatives[0].transcript, "so tell me");
            }
            other => panic!("expected Results, got {other:?}"),
        }
    }

    #[test]
    fn parses_a_final_results_message() {
        let json = r#"{"type":"Results","is_final":true,"channel":{"alternatives":[{"transcript":"so tell me about a time","confidence":0.95}]}}"#;
        let msg = parse_message(json).unwrap();
        match msg {
            DeepgramMessage::Results { is_final, .. } => assert!(is_final),
            other => panic!("expected Results, got {other:?}"),
        }
    }

    #[test]
    fn parses_speech_started_and_utterance_end_with_no_extra_fields() {
        assert_eq!(
            parse_message(r#"{"type":"SpeechStarted","timestamp":1.2}"#).unwrap(),
            DeepgramMessage::SpeechStarted
        );
        assert_eq!(
            parse_message(r#"{"type":"UtteranceEnd","last_word_end":3.4}"#).unwrap(),
            DeepgramMessage::UtteranceEnd
        );
    }

    #[test]
    fn parses_metadata() {
        assert_eq!(
            parse_message(r#"{"type":"Metadata","request_id":"abc-123"}"#).unwrap(),
            DeepgramMessage::Metadata
        );
    }

    #[test]
    fn an_unrecognized_type_is_a_readable_error_not_a_panic() {
        let result = parse_message(r#"{"type":"SomeFutureMessageType"}"#);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("could not parse"));
    }

    #[test]
    fn malformed_json_is_a_readable_error_not_a_panic() {
        let result = parse_message("not json");
        assert!(result.is_err());
    }
}
