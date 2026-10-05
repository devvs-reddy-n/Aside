/**
 * Made-up backend answers for the development preview (`?demo`). Nothing here
 * ships in the real app: `invoke.ts` only loads this file when running in a
 * plain browser tab in development.
 */
import type { InvokeResult } from "./invoke";
import type {
  CopilotSummary,
  PastSessionSummary,
  SessionAnswer,
  StoredDocument,
  TranscriptTurn,
} from "./types";

const now = Date.now();
const DAY = 86_400_000;

let copilots: CopilotSummary[] = [
  {
    id: "c1", name: "Nimbus platform interview", company: "Nimbus", target_role: "Senior Platform Engineer",
    job_description: "Own the reliability of our multi-region API platform. Work with product teams on rate limiting, observability and incident response.\n\nWhat you'll do:\n- Design and run the global traffic layer across three regions\n- Lead incident reviews and turn them into lasting fixes\n- Build the tooling that keeps deploys safe at 400 releases a week\n\nYou have eight or more years running large distributed systems, strong Go or Rust, and you explain trade-offs clearly to people outside engineering.",
    instructions: "Keep answers under a minute. Lead with the outcome, then the how. Mention the payments migration when reliability comes up, and keep numbers concrete. Avoid jargon unless they use it first.", save_transcript: true,
    llm_provider: "open_ai", llm_model: "gpt-5.6-luna", created_at_ms: now - 6 * DAY, updated_at_ms: now - 2 * 3_600_000,
    document_count: 3, has_resume: true,
  },
  {
    id: "c2", name: "Harbor design screen", company: "Harbor", target_role: "Staff Product Designer",
    job_description: "Lead design for the payments experience.", instructions: null, save_transcript: true,
    llm_provider: "open_ai", llm_model: "gpt-5.6-luna", created_at_ms: now - 12 * DAY, updated_at_ms: now - 3 * DAY,
    document_count: 1, has_resume: true,
  },
  {
    id: "c3", name: "Kite recruiter call", company: "Kite", target_role: null, job_description: null, instructions: null,
    save_transcript: false, llm_provider: "open_ai", llm_model: "gpt-5.6-terra", created_at_ms: now - 20 * DAY, updated_at_ms: now - 9 * DAY,
    document_count: 0, has_resume: false,
  },
];

const doc = (id: string, type: string, file: string, days: number, bytes: number, original = true): StoredDocument => ({
  id, session_id: null, doc_type: type, file_name: file, raw_text: `Parsed text of ${file}.\n\nExperience\nSenior Platform Engineer, 2019 – now. Led a team of six that moved the API tier to multiple regions and cut p99 latency by 38%.\n\nSkills\nGo, Rust, Kubernetes, Postgres, observability.`,
  original_available: original, mime_type: file.endsWith(".pdf") ? "application/pdf" : "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  byte_size: bytes, created_at_ms: now - days * DAY,
});

let documents: StoredDocument[] = [
  doc("d1", "resume", "Devendra-Resume-2026.pdf", 1, 184_000),
  doc("d2", "resume", "Devendra-Resume-Platform.docx", 8, 96_000),
  doc("d3", "other", "Nimbus-architecture-notes.pdf", 3, 412_000),
  doc("d4", "other", "STAR-stories.docx", 15, 54_000, false),
];

let sessions: PastSessionSummary[] = [
  { id: "s1", title: "Senior Platform Engineer", started_at_ms: now - 3 * 3_600_000, ended_at_ms: now - 2 * 3_600_000, duration_ms: 47 * 60_000, status: "ended", company: "Nimbus", segment_count: 86 },
  { id: "s2", title: "Staff Product Designer", started_at_ms: now - 3 * DAY, ended_at_ms: now - 3 * DAY + 32 * 60_000, duration_ms: 32 * 60_000, status: "ended", company: "Harbor", segment_count: 54 },
  { id: "s3", title: "Recruiter intro", started_at_ms: now - 9 * DAY, ended_at_ms: now - 9 * DAY + 12 * 60_000, duration_ms: 12 * 60_000, status: "ended", company: "Kite", segment_count: 0 },
];

const transcript: TranscriptTurn[] = [
  { speaker: "them", text: "Thanks for making time today. Tell me about a system you're proud of.", start_ms: 0, end_ms: 6000 },
  { speaker: "you", text: "Sure. I led the move of our API tier to three regions, which cut p99 latency by 38 percent.", start_ms: 9000, end_ms: 16000 },
  { speaker: "them", text: "How would you design a rate limiter for a public API that works across many servers?", start_ms: 40_000, end_ms: 47_000 },
  { speaker: "you", text: "A token bucket per key, with the counters in Redis updated by one atomic script.", start_ms: 52_000, end_ms: 60_000 },
];
const answers: SessionAnswer[] = [
  { question: "How would you design a rate limiter?", answer: "I'd use a token bucket per API key, with counters in Redis updated atomically so servers never race.\n\nKey steps:\n- Identify the caller\n- Refill on a schedule\n- Reject with 429 and Retry-After", at_ms: 50_000 },
];

const ok = <T,>(value: T): InvokeResult<T> => ({ ok: true, value });
const wait = <T,>(value: T, ms = 120): Promise<InvokeResult<T>> => new Promise((resolve) => setTimeout(() => resolve(ok(value)), ms));

let engine = "parakeet_local";
let stealth = true;
let prefs = { zoom_pct: 100, opacity_pct: 90, font_px: 13, language: "en" };
let rules = { text: "You are Aside, a quiet interview copilot. Answer in the first person, in plain spoken language...".padEnd(800, " Keep it natural."), defaultText: "default", isDefault: false, charCount: 800, meetsCacheFloor: true };

export async function mockInvoke<T>(command: string, args?: Record<string, unknown>): Promise<InvokeResult<T>> {
  const id = (args?.copilotId ?? args?.documentId ?? args?.sessionId) as string | undefined;
  const result = (): unknown => {
    switch (command) {
      case "list_copilots": return copilots;
      case "get_copilot_documents": return documents.filter((d) => (id === "c1" ? ["d1", "d3"].includes(d.id) : id === "c2" ? ["d2"].includes(d.id) : false));
      case "create_copilot": {
        const input = args?.input as Record<string, unknown>;
        const created = { ...(input as object), id: `c${copilots.length + 1}`, created_at_ms: Date.now(), updated_at_ms: Date.now(), document_count: 0, has_resume: Boolean(input.resume_id) } as unknown as CopilotSummary;
        copilots = [created, ...copilots];
        return created;
      }
      case "update_copilot": {
        const input = args?.input as Record<string, unknown>;
        copilots = copilots.map((c) => (c.id === id ? ({ ...c, ...(input as object), updated_at_ms: Date.now() } as CopilotSummary) : c));
        return copilots.find((c) => c.id === id);
      }
      case "delete_copilot": copilots = copilots.filter((c) => c.id !== id); return true;
      case "start_copilot_call": return { session: { id: "demo-session", title: "Demo", mode_id: null, participants: [], started_at_ms: Date.now(), ended_at_ms: null, status: "active", created_at_ms: Date.now(), updated_at_ms: Date.now(), company: "Nimbus", job_description: null, session_type: "interview", save_transcript: true } };
      case "list_doc_types": return ["resume", "job_description", "other"];
      case "list_recent_documents": return documents.filter((d) => d.doc_type === args?.docType);
      case "ingest_document": {
        const path = String(args?.path ?? "file.pdf");
        const created = doc(`d${documents.length + 10}`, String(args?.docType), path.split("/").pop() ?? "file.pdf", 0, 120_000);
        documents = [created, ...documents];
        return { document_id: created.id, file_name: created.file_name, doc_type: created.doc_type, char_count: 4000, chunks_queued: 3 };
      }
      case "ingest_text_document": {
        const created = doc(`d${documents.length + 10}`, String(args?.docType), `${String(args?.name)}.txt`, 0, String(args?.text ?? "").length, false);
        created.raw_text = String(args?.text ?? "");
        documents = [created, ...documents];
        return { document_id: created.id, file_name: created.file_name, doc_type: created.doc_type, char_count: created.raw_text.length, chunks_queued: 1 };
      }
      case "delete_document": documents = documents.filter((d) => d.id !== id); return true;
      case "read_document_original": return { mime_type: "application/pdf", bytes_base64: "" };
      case "fetch_job_post_text": return "Senior Platform Engineer\nWe are hiring an engineer to own the reliability of our API platform.\nResponsibilities: rate limiting, observability, incident response.";
      case "get_context_preview": return { system_char_count: 9000, token_estimate: 2400, meets_cache_floor: true, sources: [] };
      case "list_past_sessions": return sessions;
      case "get_session_transcript": return id === "s3" ? [] : transcript;
      case "get_session_answers": return id === "s3" ? [] : answers;
      case "get_session_context": return id === "s3" ? [] : ["Interviewer opened with a system-design question about rate limiting.", "Candidate proposed a token bucket backed by Redis."];
      case "summarize_past_session": return { paragraph: "A 47-minute interview covering platform reliability. The candidate described a multi-region migration and a distributed rate limiter design.", highlights: ["Multi-region migration cut p99 latency by 38%", "Rate limiter: token bucket per key, counters in Redis", "Follow-up requested on incident response"] };
      case "search_transcripts": {
        const q = String(args?.query ?? "").toLowerCase();
        return transcript.filter((t) => t.text.toLowerCase().includes(q)).map((t) => ({ session_id: "s1", session_title: "Senior Platform Engineer", speaker: t.speaker, text: t.text, start_ms: t.start_ms }));
      }
      case "delete_session": sessions = sessions.filter((s) => s.id !== id); return true;
      case "list_stt_engines": return ["speechmatics", "parakeet_local"];
      case "get_stt_engine": return engine;
      case "set_stt_engine": engine = String(args?.engine); return null;
      case "has_stt_api_key": return false;
      case "set_stt_api_key": return null;
      case "is_parakeet_model_downloaded": return true;
      case "download_parakeet_model": return null;
      case "list_llm_providers": return ["open_ai"];
      case "list_llm_models": return ["gpt-5.6-luna", "gpt-5.6-terra", "gpt-5.5", "gpt-5.4", "gpt-5.4-mini"];
      case "get_llm_provider": return "open_ai";
      case "get_llm_model": return "gpt-5.6-luna";
      case "set_llm_provider": case "set_llm_model": case "set_llm_api_key": case "set_llm_service_tier": return null;
      case "has_llm_api_key": return true;
      case "get_llm_service_tier": return "";
      case "get_stealth": return stealth;
      case "set_stealth": stealth = Boolean(args?.enabled); return stealth;
      case "get_active_session": return null;
      case "get_overlay_prefs": return prefs;
      case "set_overlay_prefs": prefs = args?.prefs as typeof prefs; return prefs;
      case "get_answer_rules": return rules;
      case "set_answer_rules": rules = { ...rules, text: String(args?.text), isDefault: false }; return rules;
      case "reset_answer_rules": rules = { ...rules, isDefault: true }; return rules;
      case "start_mic_capture": case "start_system_audio_capture": return { sample_rate: 48000, channels: 1, device_name: "Demo device", device_id: null };
      case "get_mic_device": case "get_system_device": return null;
      case "open_overlay": case "hide_main_window": case "show_main_window": case "toggle_on_screen": case "request_quit": case "confirm_quit": return null;
      default: return null;
    }
  };
  return wait(result() as T);
}
