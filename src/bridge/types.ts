/**
 * TypeScript shapes that mirror the Rust backend exactly (command
 * arguments, return values, and event payloads). Every field name and
 * casing here was verified against `src-tauri/src` directly, not guessed
 * from the old frontend — Tauri turns a Rust command's `snake_case`
 * parameters into `camelCase` on the JavaScript side, but the data
 * *inside* structs keeps whatever `serde` attribute the Rust struct uses
 * (usually `snake_case` for plain field names, sometimes a `rename_all` on
 * an enum). Each block below cites the Rust source it mirrors.
 */

// --- Audio channels (`src-tauri/src/audio/*.rs`, `commands/audio.rs`) ---

/** Which of the two ears a reading came from. Rust: `audio::channel::Channel`,
 *  `#[serde(rename_all = "lowercase")]`. */
export type ChannelId = "mic" | "system";

/** Mirrors `audio::mic::InputDevice`. */
export interface InputDeviceInfo {
  id: string | null;
  name: string;
  is_default: boolean;
}

/** Mirrors `audio::system::OutputDevice`. */
export interface OutputDeviceInfo {
  id: string | null;
  name: string;
  is_default: boolean;
  /** False when this device cannot actually be captured (no input side to
   *  tap) — surfaced rather than hidden so "why isn't my device here" has
   *  an answer. */
  tappable: boolean;
}

export type AudioDeviceInfo = InputDeviceInfo | OutputDeviceInfo;

/** Mirrors `audio::system::CaptureInfo`, the live stream info returned by
 *  `start_mic_capture` / `start_system_audio_capture` / the two
 *  `get_*_capture_info` queries. */
export interface CaptureInfo {
  sample_rate: number;
  channels: number;
  device_name: string;
  device_id: string | null;
}

/** Mirrors `commands::capture::CaptureStatus`. */
export interface CaptureStatus {
  mic_capturing: boolean;
  mic_transcribing: boolean;
  system_capturing: boolean;
  system_transcribing: boolean;
}

/** Event payload for `mic-level` / `system-level`. Rust:
 *  `audio::channel::ChannelPipeline::emit` sends `{ channel, rms }`. */
export interface ChannelLevelEvent {
  channel: ChannelId;
  rms: number;
}

/** Mirrors `audio::vad::VadState`, `#[serde(rename_all = "snake_case")]`. */
export type VadStateName = "idle" | "speech" | "hangover";

/** Event payload for `mic-vad-state` / `system-vad-state`. Rust:
 *  `audio::channel::VadStatus`. */
export interface VadStatus {
  channel: ChannelId;
  state: VadStateName;
  speaking: boolean;
  enter_threshold: number;
  noise_floor: number;
}

// --- Speech-to-text engines (`commands/stt.rs`, `stt/mod.rs`) ---

/** Mirrors `stt::EngineKind`, `#[serde(rename_all = "snake_case")]`. Only
 *  `ParakeetLocal` and `Speechmatics` are ever offered by
 *  `list_stt_engines` — Deepgram is a real, working engine kept dormant on
 *  purpose (owner decision, 2026-08-24); do not add it to any picker built
 *  from this type. */
export type EngineKind = "parakeet_local" | "deepgram" | "speechmatics";

/** Event payload for `mic-transcript` / `system-transcript`. Rust:
 *  `pipeline::TranscriptEvent`. */
export interface TranscriptEvent {
  text: string;
  is_final: boolean;
}

/** A committed turn in the live two-sided conversation. */
export interface LiveTranscriptTurn {
  channel: ChannelId;
  text: string;
}

// --- LLM / answers (`commands/llm.rs`, `llm/mod.rs`) ---

/** Mirrors `llm::LlmProviderKind`, `#[serde(rename_all = "snake_case")]`.
 *  Only `open_ai` is ever offered by `list_llm_providers` today. */
export type LlmProviderKind = "open_ai" | "anthropic" | "gemini" | "groq";

export interface AnswerStartedEvent {
  question: string;
}

export interface AnswerDeltaEvent {
  question: string;
  text: string;
}

export interface AnswerDoneEvent {
  question: string;
  /** True when every configured provider failed and the shown text was a
   *  canned fallback message, not a real model reply. */
  canned: boolean;
}

export interface AnswerErrorEvent {
  question: string;
  message: string;
}

export interface QuestionCleanedEvent {
  question: string;
  cleaned_question: string;
}

// --- Sessions (`domain/types.rs`, `commands/session.rs`, `active_session.rs`) ---

/** Mirrors `domain::types::SessionStatus`, `#[serde(rename_all = "snake_case")]`. */
export type SessionStatus = "draft" | "active" | "ended";

/** Mirrors `domain::types::SessionType`, `#[serde(rename_all = "snake_case")]`. */
export type SessionType = "interview" | "regular_call";

/** Mirrors `domain::types::Session`, the full stored row. */
export interface Session {
  id: string;
  title: string;
  mode_id: string | null;
  participants: string[];
  started_at_ms: number;
  ended_at_ms: number | null;
  status: SessionStatus;
  created_at_ms: number;
  updated_at_ms: number;
  company: string | null;
  job_description: string | null;
  session_type: SessionType;
  save_transcript: boolean;
}

// --- Reusable Copilots ---

export interface Copilot {
  id: string;
  name: string;
  company: string | null;
  target_role: string | null;
  job_description: string | null;
  instructions: string | null;
  save_transcript: boolean;
  llm_provider: LlmProviderKind;
  llm_model: string;
  created_at_ms: number;
  updated_at_ms: number;
}

export interface CopilotSummary extends Copilot {
  document_count: number;
  has_resume: boolean;
}

export interface CopilotInput {
  name: string;
  company: string | null;
  target_role: string | null;
  job_description: string | null;
  instructions: string | null;
  save_transcript: boolean;
  llm_provider: LlmProviderKind;
  llm_model: string;
  resume_id: string | null;
  document_ids: string[];
}

/** Mirrors `active_session::ActiveSessionInfo`, the lightweight "what's
 *  live right now" read `get_active_session` returns. */
export interface ActiveSessionInfo {
  id: string | null;
  session_type: SessionType;
  company: string | null;
  job_description: string | null;
  instructions: string | null;
  /** Whether this live call stores its transcript. Kept here so restarting
   *  either audio channel from the overlay preserves the call's choice. */
  save_transcript: boolean;
}

// --- Documents (`ingest/mod.rs`, `commands/documents.rs`, `storage/documents.rs`) ---

/** Mirrors `ingest::DocType`, `#[serde(rename_all = "snake_case")]`. */
export type DocType = "resume" | "job_description" | "other";

/** Mirrors `commands::documents::IngestedDocument`, returned by
 *  `ingest_document` / `ingest_text_document`. */
export interface IngestedDocument {
  document_id: string;
  file_name: string;
  doc_type: DocType;
  char_count: number;
  chunks_queued: number;
}

/** Mirrors `storage::documents::StoredDocument`, one row from
 *  `list_recent_documents`. */
export interface StoredDocument {
  id: string;
  session_id: string | null;
  doc_type: string;
  file_name: string;
  raw_text: string;
  original_available: boolean;
  mime_type: string | null;
  byte_size: number | null;
  created_at_ms: number;
}

export interface DocumentOriginal {
  mime_type: string;
  bytes_base64: string;
}

// --- Conversation window / memory (`window_memory/*.rs`) ---

/** Mirrors `window_memory::PressLog`, one Command+Enter's full record.
 *  Fields carried here for completeness even though today's screens only
 *  read a few of them off `WindowPreview`. `memory_before`/`memory_after`
 *  collapsed into one `memory_snapshot` field (2026-08-29 layered-memory
 *  rewrite) — memory updates now happen asynchronously in the background,
 *  after the press this log entry describes, so "before/after this press"
 *  stopped being a meaningful distinction. `context_error` is gone with it:
 *  the main answer model no longer emits a `context_update` that could fail
 *  to parse. */
export interface PressLog {
  press_number: number;
  window_start_ms: number;
  window_end_ms: number;
  raw_window: string;
  memory_snapshot: string;
  input_token_estimate: number;
  output_token_estimate: number;
  generated_answer: string;
  ttft_ms: number | null;
  total_ms: number | null;
}

/** Mirrors `window_memory::WindowPreview`, the Memory layer's whole
 *  reason for existing: `get_window_preview` and the `window-memory`
 *  event both send this exact shape. A prior two-layer "recent" (last 10
 *  windows) + "historical" (re-consolidated older summary) design was
 *  replaced 2026-08-29 by a single growing list: `summary` holds one 1-2
 *  sentence prose entry per completed conversation window, oldest first,
 *  appended for the whole session with no cap or re-condensing — written by
 *  an async background model. `facts` is a small, separate set of
 *  DB-backed key/value facts (visa status, notice period, ...), not window
 *  summaries. See `window_memory::background`'s doc comment for why the
 *  background compressor never sees Aside's own suggested answer, only the
 *  real transcript. */
export interface WindowPreview {
  raw_window: string;
  facts: string[];
  summary: string[];
  press_count: number;
  last_log: PressLog | null;
}

/** Mirrors `window_memory::prompt::ContextPreviewSource`, one row of the
 *  Documents layer's readiness breakdown. */
export interface ContextPreviewSource {
  label: string;
  char_count: number;
}

/** Mirrors `window_memory::prompt::ContextPreview`, returned by
 *  `get_context_preview`. `meets_cache_floor` is the one field the
 *  Documents layer's plain-English readiness readout is built from. */
export interface ContextPreview {
  system_char_count: number;
  token_estimate: number;
  meets_cache_floor: boolean;
  sources: ContextPreviewSource[];
}

// --- Answer timing (`llm/timing.rs`) ---

/** Mirrors `llm::timing::TimingSummary`, the payload of the `answer-timing`
 *  event fired once after every answer. Every field is `Option` where the
 *  underlying measurement can legitimately be missing (e.g. a failed
 *  answer never got a `ttft_ms`) — the Speed and cost layer must treat a
 *  missing value as "never happened", not as zero. `input_tokens` and
 *  `cached_input_tokens` are only present when the provider's own usage
 *  object reported them; not every provider does. */
export interface TimingSummary {
  freeze_ms: number | null;
  context_ms: number | null;
  body_ready_ms: number | null;
  headers_ms: number | null;
  first_sse_ms: number | null;
  first_model_text_ms: number | null;
  first_visible_from_request_ms: number | null;
  ttfb_ms: number | null;
  ttft_ms: number | null;
  total_ms: number | null;
  provider: string | null;
  model: string | null;
  context_chars: number;
  input_tokens: number | null;
  cached_input_tokens: number | null;
  reasoning_tokens: number | null;
  output_tokens: number | null;
}

// --- Past sessions (`commands/past_sessions.rs`, `storage/past_sessions.rs`) ---

/** Mirrors `domain::types::SessionStatus` again, restated here because
 *  `storage::past_sessions::PastSessionSummary` carries its own copy of
 *  the field rather than reusing the `Session` struct. Same values as
 *  `SessionStatus` above. */
export type PastSessionStatus = SessionStatus;

/** Mirrors `storage::past_sessions::PastSessionSummary`, one row of the
 *  Past Sessions list. `segment_count` counts only final transcript
 *  segments, so a session with nothing but interim rows (or none at all)
 *  reads as empty rather than as having real recorded speech. */
export interface PastSessionSummary {
  id: string;
  title: string;
  started_at_ms: number;
  ended_at_ms: number | null;
  duration_ms: number | null;
  status: PastSessionStatus;
  company: string | null;
  segment_count: number;
}

/** Mirrors `commands::past_sessions::Speaker`,
 *  `#[serde(rename_all = "snake_case")]`. `you` is this app's owner,
 *  `them` is whoever else was on the call. */
export type Speaker = "you" | "them";

/** Mirrors `commands::past_sessions::TranscriptTurn`, one line of a
 *  rendered transcript from `get_session_transcript`. Only ever built
 *  from `is_final = 1` rows — interim lines never made it into this. */
export interface TranscriptTurn {
  speaker: Speaker;
  text: string;
  start_ms: number;
  end_ms: number;
}

/** Mirrors `commands::past_sessions::SessionAnswer`, one AI answer
 *  generated during the session, from `get_session_answers` — interleaved
 *  with `TranscriptTurn`s by `at_ms` to build the combined transcript view
 *  (2026-08-30, owner request, matching the ParakeetAI export's own
 *  "conversation + AI answers together" shape). `answer` is the AI's own
 *  suggested answer, never what the owner actually said. */
export interface SessionAnswer {
  question: string;
  answer: string;
  at_ms: number;
}

/** A factual, on-demand overview of an already-recorded call. Generated
 *  from the final transcript by `summarize_past_session`; it is separate
 *  from the small rolling context notes saved while a call is live. */
export interface SessionSummary {
  paragraph: string;
  highlights: string[];
}

/** Mirrors `commands::past_sessions::TranscriptSearchResult`, one hit from
 *  `search_transcripts`. */
export interface TranscriptSearchResult {
  session_id: string;
  session_title: string;
  speaker: Speaker;
  text: string;
  start_ms: number;
}

// --- Answer rules (Settings > Advanced) ---

/** Mirrors the Rust `AnswerRules` struct returned by `get_answer_rules`,
 *  `set_answer_rules` and `reset_answer_rules` (agreed contract, backend
 *  built alongside this layer). `meetsCacheFloor` in camelCase per the
 *  agreed TypeScript shape, unlike every other struct in this file, which
 *  otherwise mirrors Rust's own `snake_case` field names — this type was
 *  specified directly in camelCase by the contract, not derived from a
 *  `serde` default. */
export interface AnswerRules {
  text: string;
  defaultText: string;
  isDefault: boolean;
  charCount: number;
  meetsCacheFloor: boolean;
}

// --- Live overlay (`commands/overlay.rs`) ---------------------------------

/** Mirrors `commands::overlay::OverlayPrefs` — the More menu's Zoom/
 *  Opacity/Font-size steppers (spec §9 rows 3-5), persisted locally, not
 *  per session. `opacity_pct` can never come back below 55 — enforced on
 *  the Rust side (`clamp_prefs`), not just by the stepper's own min/max. */
export interface OverlayPrefs {
  zoom_pct: number;
  opacity_pct: number;
  font_px: number;
  /** ISO 639-1 code (e.g. `"en"`) — only affects Speechmatics; Parakeet is a
   *  fixed local model with no runtime language switch. */
  language: string;
}

// --- Overlay window modes (`commands/overlay.rs`, added with the Dock redesign) ---

/** Mirrors `commands::overlay::OverlayMode`, `#[serde(rename_all = "lowercase")]`. */
export type OverlayMode = "dock" | "capsule";

/** Payload of `commands::copilots::start_copilot_call`. */
export interface StartedCopilotCall {
  session: Session;
}
