//! Speechmatics Real-Time protocol — URL, auth, message shapes, no socket.
//!
//! Deliberately the same split as `deepgram.rs`: everything here is pure and
//! unit-testable with no network, and `speechmatics_live.rs` owns the actual
//! WebSocket. Getting a protocol detail wrong is the expensive failure mode
//! for a streaming engine (it shows up as "no transcripts, no error"), so the
//! parts that can be tested offline are.
//!
//! Chosen because it is the engine ParakeetAI itself uses — their privacy
//! policy names Speechmatics directly — and matching that product's transcript
//! quality is the actual acceptance criterion here.
//!
//! ## Three ways this differs from Deepgram, each a real trap
//!
//! 1. **Config travels in a message, not the URL.** Deepgram encodes
//!    everything as query parameters; here the URL is essentially static and
//!    the whole configuration goes in `StartRecognition`. That also means the
//!    server can reject the config *after* a successful handshake.
//! 2. **Audio must not be sent until `RecognitionStarted` arrives.** The docs
//!    are explicit ("You should wait for this message before sending audio"),
//!    and sending early earns a `protocol_error` that terminates the session.
//!    Deepgram needs no such gate, so this is genuinely new state.
//! 3. **`EndOfStream` is permanent.** It is *not* Deepgram's `Finalize`. After
//!    it, the docs state any further audio "will be ignored" and the server
//!    closes. The soft equivalent — "give me a final for what you have so
//!    far", which is what `SttEngine::flush` actually means — is
//!    `ForceEndOfUtterance`. `pipeline.rs` calls `flush()` when transcription
//!    **stops**, not on speaking-light VAD. Confusing ForceEndOfUtterance
//!    with `EndOfStream` would still kill the session if anyone wired VAD
//!    flush back in.
//!
//! All message shapes below are taken from the real published corpus at
//! <https://docs.speechmatics.com/llms-full.txt> (fetched 2026-08-24) and the
//! rendered reference at
//! <https://docs.speechmatics.com/api-ref/realtime-transcription-websocket>.

/// Speechmatics' regional entry points.
///
/// The owner’s portal (2026-08-25) is pinned to **US1**, with AU1 batch-only.
/// Connecting a US1 key to `eu2.rt` (the official SDK default) or to
/// `global.rt` (which may land in another region) upgrades the websocket
/// (HTTP 101) and then rejects the key in-band as `not_authorised`.
/// [`SpeechmaticsConfig::default`] therefore pins [`Region::Us`].
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    /// Auto-routes to the nearest region. Not used: this account is US1.
    Global,
    /// Documented EU hostname (`eu.rt`).
    Eu,
    /// Official Python/JS SDK default. Wrong for a US1 portal account.
    Eu2,
    /// US realtime (`us.rt`). Matches the portal’s US1 region.
    Us,
}

impl Region {
    pub fn url(self) -> &'static str {
        match self {
            // No trailing slash: the JS SDK builds `.../v2?jwt=...`. A trailing
            // slash becomes `.../v2/?jwt=...`, which is a different path.
            Region::Global => "wss://global.rt.speechmatics.com/v2",
            Region::Eu => "wss://eu.rt.speechmatics.com/v2",
            Region::Eu2 => "wss://eu2.rt.speechmatics.com/v2",
            Region::Us => "wss://us.rt.speechmatics.com/v2",
        }
    }

    /// Value for the (deprecated-but-still-sent) `region` field on a
    /// management-platform JWT mint. Python JWTAuth accepts `eu` or `usa`.
    pub fn jwt_region(self) -> &'static str {
        match self {
            Region::Us | Region::Global => "usa",
            Region::Eu | Region::Eu2 => "eu",
        }
    }
}

/// The API-validated bounds on `max_delay`, from the docs' configuration
/// table: "Valid range: 0.7-4.0 seconds". Checked locally so a bad value is a
/// clear local error instead of an `invalid_config` that kills the session
/// after a successful connect.
pub const MAX_DELAY_MIN: f32 = 0.7;
pub const MAX_DELAY_MAX: f32 = 4.0;
/// Default max_delay for the **auto-answer** path. Must stay **above**
/// [`EOU_SILENCE_TRIGGER`] so the server's end-of-utterance can fire first.
pub const DEFAULT_MAX_DELAY: f32 = 0.9;
/// Auto-answer / Layer 9: fast interviewer stop. Not a notebook line break.
pub const EOU_SILENCE_TRIGGER: f32 = 0.6;
/// Layer 3 notebook: owner-chosen 1.5s think-pause stays on the same line.
pub const NOTEBOOK_EOU_SILENCE_TRIGGER: f32 = 1.5;
/// Must stay **above** [`NOTEBOOK_EOU_SILENCE_TRIGGER`]. Captioning-style
/// word lock — still not `is_final`. Top of the 0.7–4.0 range so EndOfUtterance
/// at 1.5s is not racing the last locked words.
pub const NOTEBOOK_MAX_DELAY: f32 = 4.0;
/// Speechmatics documents EOU as 0–2 seconds inclusive.
pub const EOU_SILENCE_MIN: f32 = 0.0;
pub const EOU_SILENCE_MAX: f32 = 2.0;

/// What a connection needs to know before it opens.
pub struct SpeechmaticsConfig {
    pub region: Region,
    pub language: String,
    /// `standard` or `enhanced`. (`melia-1` exists but is batch-only.)
    /// Default is `standard` — the owner asked to stop billing Enhanced (2026-08-25).
    pub model: String,
    /// Seconds between the end of a spoken word and its final transcript.
    pub max_delay: f32,
    /// Silence before `EndOfUtterance` (0–2s). Notebook uses 1.5; answers 0.6.
    pub eou_silence_trigger: f32,
    /// `flexible` lets smart formatting finish a number or date before
    /// committing; `fixed` cuts at exactly `max_delay`. The docs' own worked
    /// example shows `fixed` turning "I am thirty five" into "I am 30 five.",
    /// which is precisely the kind of mangling this project is trying to get
    /// *away* from — hence `flexible`.
    pub max_delay_mode: String,
    pub enable_partials: bool,
    /// Regional spelling. The docs recommend setting it for English:
    /// without one, "spelling may be inconsistent within the same transcript".
    pub output_locale: Option<String>,
    /// The rate declared in `audio_format`, which every pushed buffer must be
    /// resampled to. Fixed for the life of the session, which is exactly why
    /// it cannot just be whatever the capture device happens to report — see
    /// `speechmatics_live.rs`'s `push_audio`.
    pub sample_rate: u32,
    /// Extra words the engine should prefer (company name, tech from the
    /// job post). Sent as Speechmatics `additional_vocab`. Empty by default.
    pub additional_vocab: Vec<String>,
}

impl Default for SpeechmaticsConfig {
    fn default() -> Self {
        Self {
            region: Region::Us,
            language: "en".to_string(),
            model: "standard".to_string(),
            // The floor. This is an interview copilot: the answer is only
            // useful while the question is still hanging in the air.
            max_delay: DEFAULT_MAX_DELAY,
            eou_silence_trigger: EOU_SILENCE_TRIGGER,
            max_delay_mode: "flexible".to_string(),
            enable_partials: true,
            output_locale: Some("en-US".to_string()),
            // 16kHz clears the docs' 12kHz threshold for the "broadcast"
            // acoustic model (below it you get the telephony model), at a
            // third of 48kHz's bandwidth.
            sample_rate: 16_000,
            additional_vocab: Vec::new(),
        }
    }
}

impl SpeechmaticsConfig {
    /// Notebook / diagnostic layers (`auto_answer: false`): wait out a
    /// 1.5-second mid-thought pause instead of starting a new line at 0.6s.
    pub fn for_notebook() -> Self {
        Self {
            max_delay: NOTEBOOK_MAX_DELAY,
            eou_silence_trigger: NOTEBOOK_EOU_SILENCE_TRIGGER,
            ..Self::default()
        }
    }
}

/// Management-platform URL that turns a long-lived API key into a short-lived
/// realtime JWT. Query `type=rt` is load-bearing: the default type is `batch`,
/// and a batch token is rejected by the realtime gateway as `not_authorised`.
pub const JWT_MINT_URL: &str = "https://mp.speechmatics.com/v1/api_keys?type=rt";
/// One hour. Interviews run longer than the Python SDK's 60s default; the
/// live engine remints before this expires.
pub const JWT_TTL_SECS: u64 = 3600;

/// The connect URL without a session token. Unlike Deepgram's, this carries
/// no transcription config — only the region — because everything else goes
/// in `StartRecognition`.
pub fn build_url(config: &SpeechmaticsConfig) -> String {
    config.region.url().to_string()
}

/// Realtime websocket URL. A minted JWT is also placed in `jwt=`, matching
/// the official JS client. A long-lived portal key is **not** — that query
/// slot is for JWTs, and stuffing a raw key there is how a valid US1 key
/// can fail as a retryable handshake error instead of authenticating via
/// `Authorization: Bearer`.
pub fn build_session_url(config: &SpeechmaticsConfig, session_token: &str) -> String {
    let base = build_url(config);
    if looks_like_jwt(session_token) {
        format!("{base}?jwt={}", query_escape(session_token))
    } else {
        base
    }
}

/// RFC 3986 unreserved stay as-is; everything else is percent-encoded so a
/// JWT (or a pasted key with `+` `/` `=`) cannot break URI parsing.
fn query_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Speechmatics uses `Authorization: Bearer <key>` — **not** Deepgram's
/// `Token <key>`.
///
/// Its own function for the same reason `deepgram::auth_header_value` is:
/// with two WebSocket engines in one codebase using two different schemes,
/// the copy-paste failure is a 401 that looks like a bad key rather than a
/// bad header.
pub fn auth_header_value(api_key: &str) -> String {
    format!("Bearer {api_key}")
}

/// True for a compact JWT (`eyJ` + two dots). Long-lived portal keys are
/// random strings; if the saved value is already a JWT we must not try to
/// mint another token *with* it.
pub fn looks_like_jwt(key: &str) -> bool {
    let k = key.trim();
    k.starts_with("eyJ") && k.bytes().filter(|&b| b == b'.').count() >= 2
}

/// Body of `POST /v1/api_keys?type=rt`. Current docs mark `region`
/// deprecated; Python JWTAuth still sends it (`eu` or `usa`), and a US1
/// portal account must not be minted as `eu`.
pub fn jwt_mint_body(ttl_secs: u64, region: Region) -> serde_json::Value {
    serde_json::json!({ "ttl": ttl_secs, "region": region.jwt_region() })
}

/// Reads `key_value` out of a management-platform mint response.
///
/// The Python SDK requires HTTP 201; some gateways have returned 200 with
/// the same JSON. Either is success. 401/403 mean the long-lived key itself
/// is wrong — that must not be retried as a websocket problem.
pub fn parse_jwt_mint_response(status: u16, body: &str) -> Result<String, String> {
    if status != 200 && status != 201 {
        let hint = match status {
            401 | 403 => {
                " Speechmatics did not accept this API key. Paste the long-lived key from the portal (Settings → API Keys), not a curl “Bearer …” line and not a batch-only token."
            }
            _ => "",
        };
        return Err(format!(
            "Speechmatics realtime-token request failed (HTTP {status}).{hint} {body}"
        ));
    }
    let value: serde_json::Value = serde_json::from_str(body).map_err(|e| {
        format!("Speechmatics realtime-token response was not JSON: {e} (body: {body})")
    })?;
    value
        .get("key_value")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            format!("Speechmatics realtime-token response had no key_value (body: {body})")
        })
}

/// Serializes the opening `StartRecognition`, validating locally first.
///
/// Local validation matters more here than it looks: an out-of-range value is
/// rejected by the server as `invalid_config` **after** the handshake has
/// already succeeded, so without this the failure arrives as a mysterious
/// mid-session disconnect rather than a clear error at the point of mistake.
pub fn start_recognition_message(config: &SpeechmaticsConfig) -> Result<String, String> {
    if !(MAX_DELAY_MIN..=MAX_DELAY_MAX).contains(&config.max_delay) {
        return Err(format!(
            "max_delay must be between {MAX_DELAY_MIN} and {MAX_DELAY_MAX} seconds, got {}",
            config.max_delay
        ));
    }
    if config.sample_rate == 0 {
        return Err("sample_rate must be greater than zero".to_string());
    }
    if config.language.trim().is_empty() {
        return Err("language must not be empty".to_string());
    }
    if !(EOU_SILENCE_MIN..=EOU_SILENCE_MAX).contains(&config.eou_silence_trigger) {
        return Err(format!(
            "end_of_utterance_silence_trigger must be between {EOU_SILENCE_MIN} and {EOU_SILENCE_MAX} seconds, got {}",
            config.eou_silence_trigger
        ));
    }
    if config.eou_silence_trigger >= config.max_delay {
        return Err(format!(
            "end_of_utterance_silence_trigger ({}) must stay below max_delay ({})",
            config.eou_silence_trigger, config.max_delay
        ));
    }

    let mut transcription = serde_json::json!({
        "language": config.language,
        "model": config.model,
        "max_delay": config.max_delay,
        "max_delay_mode": config.max_delay_mode,
        "enable_partials": config.enable_partials,
        // Nested, not a sibling of transcription_config. A top-level
        // conversation_config is rejected as protocol_error: "Additional
        // property conversation_config is not allowed" (US1 live, 2026-08-25).
        "conversation_config": {
            "end_of_utterance_silence_trigger": config.eou_silence_trigger,
        },
    });
    if let Some(locale) = &config.output_locale {
        transcription["output_locale"] = serde_json::json!(locale);
    }
    if !config.additional_vocab.is_empty() {
        transcription["additional_vocab"] = serde_json::json!(config
            .additional_vocab
            .iter()
            .filter(|w| !w.trim().is_empty())
            .map(|w| serde_json::json!({ "content": w.trim() }))
            .collect::<Vec<_>>());
    }

    let message = serde_json::json!({
        "message": "StartRecognition",
        "audio_format": {
            "type": "raw",
            "encoding": "pcm_s16le",
            "sample_rate": config.sample_rate,
        },
        "transcription_config": transcription,
    });
    serde_json::to_string(&message)
        .map_err(|e| format!("could not serialize StartRecognition: {e}"))
}

/// Company name plus distinctive words from the job post, so names the
/// interviewer will say (the company, a product, a stack) are less likely
/// to come back mangled.
pub fn vocab_from_session(company: Option<&str>, job_description: Option<&str>) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    if let Some(c) = company.map(str::trim).filter(|s| !s.is_empty()) {
        terms.push(c.to_string());
    }
    if let Some(jd) = job_description {
        for word in jd.split(|c: char| !c.is_alphanumeric()) {
            let w = word.trim();
            if w.len() < 5 {
                continue;
            }
            // Prefer words that look like names or tech (capitalized, or
            // mixed-case like GraphQL), not filler.
            let first = w.chars().next().unwrap_or(' ');
            if first.is_uppercase() || w.chars().any(|c| c.is_ascii_uppercase()) {
                let owned = w.to_string();
                if !terms.iter().any(|t| t.eq_ignore_ascii_case(&owned)) {
                    terms.push(owned);
                }
            }
            if terms.len() >= 20 {
                break;
            }
        }
    }
    terms
}

/// "Give me a final for what you have so far" — the soft finalize that
/// `SttEngine::flush` actually means. **Not** `EndOfStream`; see the module
/// doc comment for why that distinction is load-bearing.
///
/// `timestamp` is optional in the protocol and deliberately omitted: it is
/// "the number of seconds since the beginning of the audio", a quantity this
/// engine has no reason to track, and getting it wrong is worse than leaving
/// the server to use its own position.
pub fn force_end_of_utterance_message() -> &'static str {
    r#"{"message":"ForceEndOfUtterance"}"#
}

/// Ends the session for good. `last_seq_no` is required by the protocol, and
/// must be the number of `AddAudio` frames actually *sent* — see
/// `speechmatics_live.rs` for why that counter lives in the connection task
/// and not in `push_audio`.
pub fn end_of_stream_message(last_seq_no: u64) -> String {
    format!(r#"{{"message":"EndOfStream","last_seq_no":{last_seq_no}}}"#)
}

/// Server-to-client message, discriminated by the JSON `"message"` field.
///
/// Unknown variants are tolerated rather than fatal — `parse_message` maps
/// them to [`SpeechmaticsMessage::Unknown`], because a vendor adding a
/// message type must not take down a running session.
#[derive(Debug, Clone, PartialEq)]
pub enum SpeechmaticsMessage {
    RecognitionStarted,
    AudioAdded {
        seq_no: u64,
    },
    AddPartialTranscript {
        transcript: String,
    },
    AddTranscript {
        transcript: String,
    },
    EndOfUtterance,
    EndOfTranscript,
    /// Always fatal. The docs: "After any error, the transcription is
    /// terminated and the connection is closed."
    Error {
        error_type: String,
        reason: String,
    },
    /// Never fatal, but two types (`idle_timeout`, `session_timeout`) are
    /// advance notice that the server intends to close.
    Warning {
        warning_type: String,
        reason: String,
    },
    Info {
        info_type: String,
        reason: String,
    },
    /// A message type this build does not know about.
    Unknown {
        message: String,
    },
}

/// Pulls the transcript text out of a transcript message.
///
/// Reads `metadata.transcript` first and falls back to a top-level
/// `transcript`, because **the vendor's own documentation contradicts
/// itself**: the field listing presents `transcript` as top-level, while the
/// example JSON, the JavaScript quickstart (`data.metadata?.transcript`) and
/// the official Rust SDK's own struct all nest it under `metadata`. Three
/// sources against one, so nested is tried first — but a fallback costs one
/// line and turns "the vendor changed a field" from a silent
/// everything-is-blank failure into a non-event.
fn transcript_text(value: &serde_json::Value) -> String {
    value
        .get("metadata")
        .and_then(|m| m.get("transcript"))
        .and_then(|t| t.as_str())
        .or_else(|| value.get("transcript").and_then(|t| t.as_str()))
        .unwrap_or_default()
        .to_string()
}

fn string_field(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

/// Parses one server message. Malformed JSON is an error; an unrecognized
/// `message` value is not.
pub fn parse_message(json: &str) -> Result<SpeechmaticsMessage, String> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| format!("could not parse speechmatics message: {e} (body: {json})"))?;
    let Some(kind) = value.get("message").and_then(|m| m.as_str()) else {
        return Err(format!(
            "speechmatics message has no \"message\" field (body: {json})"
        ));
    };

    Ok(match kind {
        "RecognitionStarted" => SpeechmaticsMessage::RecognitionStarted,
        "AudioAdded" => SpeechmaticsMessage::AudioAdded {
            seq_no: value
                .get("seq_no")
                .and_then(|s| s.as_u64())
                .unwrap_or_default(),
        },
        "AddPartialTranscript" => SpeechmaticsMessage::AddPartialTranscript {
            transcript: transcript_text(&value),
        },
        "AddTranscript" => SpeechmaticsMessage::AddTranscript {
            transcript: transcript_text(&value),
        },
        "EndOfUtterance" => SpeechmaticsMessage::EndOfUtterance,
        "EndOfTranscript" => SpeechmaticsMessage::EndOfTranscript,
        "Error" => SpeechmaticsMessage::Error {
            error_type: string_field(&value, "type"),
            reason: string_field(&value, "reason"),
        },
        "Warning" => SpeechmaticsMessage::Warning {
            warning_type: string_field(&value, "type"),
            reason: string_field(&value, "reason"),
        },
        "Info" => SpeechmaticsMessage::Info {
            info_type: string_field(&value, "type"),
            reason: string_field(&value, "reason"),
        },
        other => SpeechmaticsMessage::Unknown {
            message: other.to_string(),
        },
    })
}

/// What to do about a closed connection.
#[derive(Debug, Clone, PartialEq)]
pub enum CloseDisposition {
    /// Reconnecting cannot help — a credential or configuration fault, or a
    /// spent quota. Surface it to the user instead of retrying silently.
    Terminal(String),
    /// Retryable, but the docs ask for a floor of 5-10 seconds on these
    /// specifically rather than an immediate exponential retry.
    RetrySlow,
    /// Ordinary network trouble — the existing exponential backoff applies.
    RetryFast,
}

/// Classifies a WebSocket close code per Speechmatics' documented table.
///
/// Encoding the vendor's retry guidance as data rather than as scattered
/// `if`s is what makes it testable — and the distinction is real: retrying a
/// `not_authorised` in a tight loop burns nothing but the user's patience,
/// while retrying `quota_exceeded` too fast is how you stay over quota.
pub fn classify_close(code: u16) -> CloseDisposition {
    match code {
        4001 => CloseDisposition::Terminal(
            "Speechmatics rejected the API key (not_authorised). Check the key in settings.".to_string(),
        ),
        4003 => CloseDisposition::Terminal(
            "This Speechmatics account is not allowed to use real-time transcription (not_allowed).".to_string(),
        ),
        4004 => CloseDisposition::Terminal(
            "Speechmatics rejected the requested model (invalid_model).".to_string(),
        ),
        4006 => CloseDisposition::Terminal(
            "The Speechmatics usage quota for this account is spent (timelimit_exceeded).".to_string(),
        ),
        // Concurrent-session limit. Retryable, but slowly — and worth knowing
        // that Aside runs mic and system audio as two separate engines, which
        // is exactly the free tier's ceiling of two.
        4005 => CloseDisposition::RetrySlow,
        4013 | 1011 => CloseDisposition::RetrySlow,
        _ => CloseDisposition::RetryFast,
    }
}

/// True for the in-band `Error` types that no amount of reconnecting fixes.
pub fn is_terminal_error_type(error_type: &str) -> bool {
    matches!(
        error_type,
        "not_authorised"
            | "not_allowed"
            | "invalid_config"
            | "invalid_audio_type"
            | "invalid_model"
            | "invalid_language"
            | "timelimit_exceeded"
            // Same StartRecognition sent again will fail the same way.
            | "protocol_error"
            | "invalid_message"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_header_uses_bearer_not_token() {
        // The direct analogue of `deepgram.rs`'s
        // `auth_header_uses_token_not_bearer`, and the reason both exist:
        // this codebase now has two WebSocket engines using two *different*
        // schemes, so copying either one's header into the other produces a
        // 401 that reads like a bad key.
        assert_eq!(auth_header_value("abc123"), "Bearer abc123");
        assert!(!auth_header_value("abc123").starts_with("Token "));
    }

    #[test]
    fn each_region_maps_to_its_documented_endpoint() {
        assert_eq!(
            build_url(&SpeechmaticsConfig::default()),
            "wss://us.rt.speechmatics.com/v2"
        );
        for region in [Region::Global, Region::Eu, Region::Eu2, Region::Us] {
            let url = region.url();
            assert!(
                url.starts_with("wss://"),
                "{url} must be a secure websocket URL"
            );
            assert!(
                url.contains(".rt.speechmatics.com/v2"),
                "{url} must target the v2 real-time API"
            );
            assert!(
                !url.ends_with('/'),
                "{url} must not use a trailing slash (jwt query is appended)"
            );
        }
        assert_ne!(Region::Eu.url(), Region::Us.url());
        assert_ne!(Region::Eu.url(), Region::Eu2.url());
    }

    #[test]
    fn session_url_puts_the_token_in_the_jwt_query() {
        let url = build_session_url(&SpeechmaticsConfig::default(), "eyJ.abc.def");
        assert_eq!(url, "wss://us.rt.speechmatics.com/v2?jwt=eyJ.abc.def");
    }

    #[test]
    fn session_url_percent_encodes_query_unsafe_characters() {
        let url = build_session_url(&SpeechmaticsConfig::default(), "eyJ.abc+def/ghi=.sig");
        assert_eq!(
            url,
            "wss://us.rt.speechmatics.com/v2?jwt=eyJ.abc%2Bdef%2Fghi%3D.sig"
        );
    }

    #[test]
    fn a_long_lived_portal_key_does_not_go_in_the_jwt_query() {
        let url = build_session_url(
            &SpeechmaticsConfig::default(),
            "wmz9fkLJM6U5NdyaG3HLHybGZj65PXp",
        );
        assert_eq!(url, "wss://us.rt.speechmatics.com/v2");
        assert!(!url.contains("jwt="));
    }

    #[test]
    fn looks_like_jwt_requires_eyj_and_two_dots() {
        assert!(looks_like_jwt("eyJhbGciOiJSUzI1NiJ9.e30.sig"));
        assert!(!looks_like_jwt("wmz9fkLJM6U5NdyaG3HLHybGZj65PXp"));
        assert!(!looks_like_jwt("eyJonlyone"));
        assert!(!looks_like_jwt(""));
    }

    #[test]
    fn jwt_mint_body_asks_for_an_rt_usa_token_on_the_default_us_region() {
        let body = jwt_mint_body(3600, Region::Us);
        assert_eq!(body["ttl"], 3600);
        assert_eq!(body["region"], "usa");
        assert_eq!(jwt_mint_body(3600, Region::Eu2)["region"], "eu");
        assert!(JWT_MINT_URL.ends_with("?type=rt"));
        assert_eq!(JWT_TTL_SECS, 3600);
        assert_eq!(Region::Us.jwt_region(), "usa");
    }

    #[test]
    fn jwt_mint_response_reads_key_value_on_201_or_200() {
        let json = r#"{"apikey_id":null,"key_value":"eyJhbG.test.sig"}"#;
        assert_eq!(
            parse_jwt_mint_response(201, json).unwrap(),
            "eyJhbG.test.sig"
        );
        assert_eq!(
            parse_jwt_mint_response(200, json).unwrap(),
            "eyJhbG.test.sig"
        );
    }

    #[test]
    fn jwt_mint_401_tells_the_user_the_saved_key_is_wrong() {
        let err = parse_jwt_mint_response(401, "unauthorized").unwrap_err();
        assert!(err.contains("HTTP 401"), "{err}");
        assert!(err.contains("long-lived key"), "{err}");
    }

    #[test]
    fn jwt_mint_response_without_key_value_is_an_error() {
        let err = parse_jwt_mint_response(201, r#"{"apikey_id":null}"#).unwrap_err();
        assert!(err.contains("key_value"), "{err}");
    }

    #[test]
    fn start_recognition_matches_the_documented_shape() {
        let json = start_recognition_message(&SpeechmaticsConfig::default())
            .expect("default config is valid");
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(v["message"], "StartRecognition");
        // Verbatim from the docs' own example.
        assert_eq!(v["audio_format"]["type"], "raw");
        assert_eq!(v["audio_format"]["encoding"], "pcm_s16le");
        assert_eq!(v["audio_format"]["sample_rate"], 16_000);
        assert_eq!(v["transcription_config"]["language"], "en");
        assert_eq!(v["transcription_config"]["model"], "standard");
        assert_eq!(v["transcription_config"]["max_delay_mode"], "flexible");
        assert_eq!(v["transcription_config"]["enable_partials"], true);
        assert_eq!(v["transcription_config"]["output_locale"], "en-US");
        assert_eq!(
            v["transcription_config"]["conversation_config"]["end_of_utterance_silence_trigger"],
            EOU_SILENCE_TRIGGER
        );
        assert!(
            v.get("conversation_config").is_none(),
            "conversation_config must nest under transcription_config, not sit at the root"
        );
        assert!(
            v["transcription_config"]["max_delay"].as_f64().unwrap() > EOU_SILENCE_TRIGGER as f64,
            "EOU trigger must be less than max_delay"
        );
        // There is deliberately no `channels` field: raw audio_format is a
        // single mono stream, and multichannel is an entirely different
        // mechanism. Asserting its absence stops someone "fixing" that.
        assert!(v["audio_format"].get("channels").is_none());
    }

    #[test]
    fn an_out_of_range_max_delay_is_rejected_locally_not_by_the_server() {
        // The server rejects these as `invalid_config` *after* a successful
        // handshake, which surfaces as an unexplained mid-session
        // disconnect. Catching it here makes it a legible error instead.
        let too_fast = SpeechmaticsConfig {
            max_delay: 0.3,
            ..SpeechmaticsConfig::default()
        };
        assert!(start_recognition_message(&too_fast).is_err());

        let too_slow = SpeechmaticsConfig {
            max_delay: 9.0,
            ..SpeechmaticsConfig::default()
        };
        assert!(start_recognition_message(&too_slow).is_err());

        // Both documented bounds are themselves valid.
        for delay in [MAX_DELAY_MIN, MAX_DELAY_MAX] {
            let ok = SpeechmaticsConfig {
                max_delay: delay,
                ..SpeechmaticsConfig::default()
            };
            assert!(
                start_recognition_message(&ok).is_ok(),
                "{delay} is a documented-valid max_delay"
            );
        }
    }

    #[test]
    fn the_default_config_asks_for_fast_finals_with_eou_below_max_delay() {
        let cfg = SpeechmaticsConfig::default();
        assert_eq!(cfg.max_delay, DEFAULT_MAX_DELAY);
        assert_eq!(cfg.eou_silence_trigger, EOU_SILENCE_TRIGGER);
        assert!(cfg.max_delay > cfg.eou_silence_trigger);
        assert!(cfg.enable_partials);
    }

    #[test]
    fn notebook_config_waits_out_a_one_and_a_half_second_pause() {
        let cfg = SpeechmaticsConfig::for_notebook();
        assert_eq!(cfg.eou_silence_trigger, NOTEBOOK_EOU_SILENCE_TRIGGER);
        assert_eq!(cfg.max_delay, NOTEBOOK_MAX_DELAY);
        assert!(cfg.eou_silence_trigger > 1.0);
        assert!(cfg.eou_silence_trigger < cfg.max_delay);
        let json = start_recognition_message(&cfg).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let eou = v["transcription_config"]["conversation_config"]
            ["end_of_utterance_silence_trigger"]
            .as_f64()
            .unwrap();
        assert!(
            (eou - 1.5).abs() < 0.01,
            "notebook EOU must be 1.5, got {eou}"
        );
        assert!(
            eou >= 1.5,
            "notebook EOU must not be the 0.6 auto-answer value"
        );
    }

    #[test]
    fn eou_at_or_above_max_delay_is_rejected_locally() {
        let illegal = SpeechmaticsConfig {
            eou_silence_trigger: 1.5,
            max_delay: 0.9,
            ..SpeechmaticsConfig::default()
        };
        let err = start_recognition_message(&illegal).unwrap_err();
        assert!(err.contains("must stay below max_delay"), "{err}");

        let equal = SpeechmaticsConfig {
            eou_silence_trigger: 2.0,
            max_delay: 2.0,
            ..SpeechmaticsConfig::default()
        };
        assert!(start_recognition_message(&equal).is_err());
    }

    #[test]
    fn additional_vocab_is_omitted_when_empty_and_present_when_set() {
        let json = start_recognition_message(&SpeechmaticsConfig::default()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            v["transcription_config"].get("additional_vocab").is_none(),
            "empty vocab must not send an empty array the server might reject"
        );

        let mut cfg = SpeechmaticsConfig::default();
        cfg.additional_vocab = vec!["Acme".into(), "Redshift".into()];
        let json = start_recognition_message(&cfg).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let vocab = v["transcription_config"]["additional_vocab"]
            .as_array()
            .expect("vocab array");
        assert_eq!(vocab[0]["content"], "Acme");
        assert_eq!(vocab[1]["content"], "Redshift");
    }

    #[test]
    fn vocab_from_session_picks_the_company_and_capitalized_job_words() {
        let terms =
            vocab_from_session(Some("Acme"), Some("We use AWS Redshift and GraphQL daily."));
        assert!(terms.iter().any(|t| t == "Acme"));
        assert!(terms.iter().any(|t| t == "Redshift"));
        assert!(terms.iter().any(|t| t == "GraphQL"));
        assert!(!terms.iter().any(|t| t == "daily"));
    }

    #[test]
    fn a_documented_add_transcript_parses_with_transcript_nested_in_metadata() {
        // This body is copied verbatim from the published API reference.
        let json = r#"{
            "message": "AddTranscript",
            "format": "2.9",
            "metadata": { "transcript": "full tell radar", "start_time": 0.11, "end_time": 1.07 },
            "results": [
                { "type": "word", "start_time": 0.11, "end_time": 0.4,
                  "alternatives": [{ "content": "full", "confidence": 0.7 }] }
            ]
        }"#;
        assert_eq!(
            parse_message(json).unwrap(),
            SpeechmaticsMessage::AddTranscript {
                transcript: "full tell radar".to_string()
            }
        );
    }

    #[test]
    fn a_top_level_transcript_field_is_still_read() {
        // The vendor's field listing documents `transcript` at the top level
        // while its example nests it. If the wire ever matches the listing,
        // this fallback is the difference between working and blank.
        let json = r#"{"message":"AddTranscript","transcript":"hello there"}"#;
        assert_eq!(
            parse_message(json).unwrap(),
            SpeechmaticsMessage::AddTranscript {
                transcript: "hello there".to_string()
            }
        );
    }

    #[test]
    fn partials_and_finals_are_distinguished() {
        let partial =
            r#"{"message":"AddPartialTranscript","metadata":{"transcript":"half a sen"}}"#;
        assert_eq!(
            parse_message(partial).unwrap(),
            SpeechmaticsMessage::AddPartialTranscript {
                transcript: "half a sen".to_string()
            }
        );
    }

    #[test]
    fn control_and_diagnostic_messages_parse() {
        assert_eq!(
            parse_message(r#"{"message":"RecognitionStarted","id":"x"}"#).unwrap(),
            SpeechmaticsMessage::RecognitionStarted
        );
        assert_eq!(
            parse_message(r#"{"message":"AudioAdded","seq_no":7}"#).unwrap(),
            SpeechmaticsMessage::AudioAdded { seq_no: 7 }
        );
        assert_eq!(
            parse_message(r#"{"message":"EndOfUtterance"}"#).unwrap(),
            SpeechmaticsMessage::EndOfUtterance
        );
        assert_eq!(
            parse_message(r#"{"message":"EndOfTranscript"}"#).unwrap(),
            SpeechmaticsMessage::EndOfTranscript
        );
        assert_eq!(
            parse_message(r#"{"message":"Error","type":"not_authorised","reason":"bad key"}"#)
                .unwrap(),
            SpeechmaticsMessage::Error {
                error_type: "not_authorised".to_string(),
                reason: "bad key".to_string()
            }
        );
    }

    #[test]
    fn an_unknown_message_type_is_tolerated_rather_than_fatal() {
        // A vendor adding a message type must not take down a live session.
        let parsed = parse_message(r#"{"message":"SomeFutureThing","x":1}"#).unwrap();
        assert_eq!(
            parsed,
            SpeechmaticsMessage::Unknown {
                message: "SomeFutureThing".to_string()
            }
        );
    }

    #[test]
    fn malformed_input_is_a_readable_error_not_a_panic() {
        assert!(parse_message("{not json").is_err());
        assert!(parse_message(r#"{"no_message_field":true}"#).is_err());
        // A transcript message missing its text is not an error — it is an
        // empty transcript, which the engine simply drops.
        assert_eq!(
            parse_message(r#"{"message":"AddTranscript"}"#).unwrap(),
            SpeechmaticsMessage::AddTranscript {
                transcript: String::new()
            }
        );
    }

    #[test]
    fn flush_maps_to_force_end_of_utterance_and_never_to_end_of_stream() {
        // The single most dangerous confusion in this protocol. `flush()` is
        // still ForceEndOfUtterance (used when transcription stops). If it
        // ever sent EndOfStream, a stop-or-reconnect would kill the session
        // and then transcribe nothing.
        let flush = force_end_of_utterance_message();
        assert!(flush.contains("ForceEndOfUtterance"));
        assert!(!flush.contains("EndOfStream"));
        let v: serde_json::Value = serde_json::from_str(flush).expect("must be valid JSON");
        assert_eq!(v["message"], "ForceEndOfUtterance");
    }

    #[test]
    fn end_of_stream_carries_the_required_sequence_number() {
        let v: serde_json::Value = serde_json::from_str(&end_of_stream_message(1234)).unwrap();
        assert_eq!(v["message"], "EndOfStream");
        assert_eq!(v["last_seq_no"], 1234);
    }

    #[test]
    fn close_codes_follow_the_documented_retry_guidance() {
        assert!(
            matches!(classify_close(4001), CloseDisposition::Terminal(_)),
            "a bad key must not be retried forever"
        );
        assert!(matches!(
            classify_close(4003),
            CloseDisposition::Terminal(_)
        ));
        assert!(matches!(
            classify_close(4004),
            CloseDisposition::Terminal(_)
        ));
        assert!(matches!(
            classify_close(4006),
            CloseDisposition::Terminal(_)
        ));
        // The docs single these three out for a 5-10s retry floor.
        assert_eq!(classify_close(4005), CloseDisposition::RetrySlow);
        assert_eq!(classify_close(4013), CloseDisposition::RetrySlow);
        assert_eq!(classify_close(1011), CloseDisposition::RetrySlow);
        // Ordinary drops keep the existing fast exponential backoff.
        assert_eq!(classify_close(1006), CloseDisposition::RetryFast);
        assert_eq!(classify_close(1000), CloseDisposition::RetryFast);
    }

    #[test]
    fn terminal_error_types_are_the_ones_no_reconnect_can_fix() {
        for t in [
            "not_authorised",
            "invalid_config",
            "invalid_audio_type",
            "invalid_language",
            "protocol_error",
            "invalid_message",
        ] {
            assert!(
                is_terminal_error_type(t),
                "{t} is a config/credential fault"
            );
        }
        // Transient server-side faults must stay retryable.
        for t in [
            "job_error",
            "internal_error",
            "idle_timeout",
            "session_timeout",
            "quota_exceeded",
        ] {
            assert!(
                !is_terminal_error_type(t),
                "{t} is transient and must be retried"
            );
        }
    }
}
