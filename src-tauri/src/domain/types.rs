use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Where a session currently stands. Kept intentionally small — grows only
/// when a real feature (capture, transcription, AI answers) needs more states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Draft,
    Active,
    Ended,
}

/// Which kind of call a session is — a fixed, two-value distinction (2026-08-25
/// setup-screen work), deliberately **not** a general custom-modes/persona
/// system (that's Phase 9's `modes` table, still unwired). Only changes one
/// line of the LLM system-context block (`llm::session_context`) today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionType {
    Interview,
    RegularCall,
}

impl Default for SessionType {
    fn default() -> Self {
        SessionType::Interview
    }
}

/// A single meeting/interview session. This is the seed of Aside's data
/// model — later phases (capture, transcripts, AI answers) attach to a
/// session by id rather than replacing this shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub title: String,
    /// Which persona/mode this session runs under. `None` until Phase 9
    /// (personas) lands and picks a default.
    pub mode_id: Option<String>,
    pub participants: Vec<String>,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
    pub status: SessionStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    /// Setup-screen fields (2026-08-25), all optional/defaulted so every
    /// existing `Session::new` call site keeps compiling unchanged.
    pub company: Option<String>,
    pub job_description: Option<String>,
    pub session_type: SessionType,
    /// Create screen's "Save Transcript" checkbox (2026-08-29 Create/Connect
    /// rebuild). Defaults `true` — every session before this field existed
    /// was, in effect, always saved. `src/layers/create/ConnectStep.tsx`
    /// reads this (via the `Session` this field round-trips to) and passes
    /// `sessionId: null` instead of the real id to `start_channel_transcription`
    /// when it's `false` — `pipeline.rs`'s whole persistence path (writing
    /// final transcript segments, indexing them for RAG) is gated on that id
    /// being `Some`, so this is what actually stops the conversation from
    /// being written to disk (2026-08-29 fix — before it, this field was
    /// saved and never consulted by anything).
    pub save_transcript: bool,
}

impl Session {
    pub fn new(title: impl Into<String>, started_at_ms: u64) -> Self {
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            mode_id: None,
            participants: Vec::new(),
            started_at_ms,
            ended_at_ms: None,
            status: SessionStatus::Draft,
            created_at_ms: started_at_ms,
            updated_at_ms: started_at_ms,
            company: None,
            job_description: None,
            session_type: SessionType::default(),
            save_transcript: true,
        }
    }
}
