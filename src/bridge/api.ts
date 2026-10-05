import { invoke, type InvokeResult } from "./invoke";
import type {
  ActiveSessionInfo,
  AnswerRules,
  AudioDeviceInfo,
  CaptureInfo,
  CaptureStatus,
  ChannelId,
  ContextPreview,
  Copilot,
  CopilotInput,
  CopilotSummary,
  DocType,
  DocumentOriginal,
  EngineKind,
  IngestedDocument,
  LiveTranscriptTurn,
  LlmProviderKind,
  ModelConnections,
  OverlayMode,
  OverlayPrefs,
  PastSessionSummary,
  Session,
  SessionAnswer,
  SessionSummary,
  StartedCopilotCall,
  StoredDocument,
  TranscriptSearchResult,
  TranscriptTurn,
  WindowPreview,
} from "./types";

export type Result<T> = InvokeResult<T>;
export type AnswerLength = "small" | "medium" | "long";

/**
 * Every backend command Aside's screens use, in one typed place. The
 * argument names here are the exact camelCase names Tauri expects; each was
 * checked against the Rust command's signature. Nothing else in `src/`
 * calls `invoke` directly.
 */

const audioCommands = (channel: ChannelId) =>
  channel === "mic"
    ? { start: "start_mic_capture", stop: "stop_mic_capture", list: "list_input_devices", get: "get_mic_device", set: "set_mic_device", info: "get_mic_capture_info" }
    : { start: "start_system_audio_capture", stop: "stop_system_audio_capture", list: "list_output_devices", get: "get_system_device", set: "set_system_device", info: "get_system_capture_info" };

export const audio = {
  devices: (channel: ChannelId) => invoke<AudioDeviceInfo[]>(audioCommands(channel).list),
  pinnedDevice: (channel: ChannelId) => invoke<string | null>(audioCommands(channel).get),
  pinDevice: (channel: ChannelId, device: string | null) => invoke<void>(audioCommands(channel).set, { device }),
  start: (channel: ChannelId, device: string | null) => invoke<CaptureInfo>(audioCommands(channel).start, { device }),
  stop: (channel: ChannelId) => invoke<void>(audioCommands(channel).stop),
  info: (channel: ChannelId) => invoke<CaptureInfo | null>(audioCommands(channel).info),
  status: () => invoke<CaptureStatus>("get_capture_status"),
  startTranscription: (channel: ChannelId, sessionId: string | null) =>
    invoke<void>("start_channel_transcription", { channel, sessionId }),
  stopTranscription: (channel: ChannelId) => invoke<void>("stop_channel_transcription", { channel }),
};

export const live = {
  activeSession: () => invoke<ActiveSessionInfo | null>("get_active_session"),
  transcript: () => invoke<LiveTranscriptTurn[]>("get_live_transcript"),
  windowPreview: () => invoke<WindowPreview>("get_window_preview"),
  answerWindow: (length: AnswerLength | null) => invoke<void>("trigger_window_answer", { length }),
  answerScreenshot: () => invoke<void>("trigger_screenshot_answer"),
  ask: (question: string, sessionId: string | null) => invoke<void>("trigger_answer", { question, sessionId }),
  end: (sessionId: string | null) => invoke<void>("end_live_session", { sessionId }),
};

export const windows = {
  openOverlay: () => invoke<void>("open_overlay"),
  hideMain: () => invoke<void>("hide_main_window"),
  showMain: () => invoke<void>("show_main_window"),
  setOverlayMode: (mode: OverlayMode, width?: number, height?: number) =>
    invoke<void>("set_overlay_mode", { mode, width: width ?? null, height: height ?? null }),
  dragResizeOverlay: (edge: Parameters<ReturnType<typeof import("@tauri-apps/api/window").getCurrentWindow>["startResizeDragging"]>[0]) => invoke<void>("drag_resize_overlay", { edge }),
  overlaySize: () => invoke<[number, number]>("get_overlay_size"),
  resizeOverlay: (width: number, height: number) => invoke<void>("resize_overlay", { width, height }),
  overlayPrefs: () => invoke<OverlayPrefs>("get_overlay_prefs"),
  saveOverlayPrefs: (prefs: OverlayPrefs) => invoke<OverlayPrefs>("set_overlay_prefs", { prefs }),
  toggleOnScreen: () => invoke<void>("toggle_on_screen"),
  requestQuit: () => invoke<void>("request_quit"),
  confirmQuit: () => invoke<void>("confirm_quit"),
};

export const stealth = {
  get: () => invoke<boolean>("get_stealth"),
  set: (enabled: boolean) => invoke<boolean>("set_stealth", { enabled }),
};

export const copilots = {
  list: () => invoke<CopilotSummary[]>("list_copilots"),
  sources: (copilotId: string) => invoke<StoredDocument[]>("get_copilot_documents", { copilotId }),
  create: (input: CopilotInput) => invoke<Copilot>("create_copilot", { input }),
  update: (copilotId: string, input: CopilotInput) => invoke<Copilot>("update_copilot", { copilotId, input }),
  remove: (copilotId: string) => invoke<boolean>("delete_copilot", { copilotId }),
  startCall: (copilotId: string): Promise<Result<StartedCopilotCall>> =>
    invoke<StartedCopilotCall>("start_copilot_call", { copilotId }),
};

export const documents = {
  types: () => invoke<DocType[]>("list_doc_types"),
  recent: (docType: DocType, limit: number) => invoke<StoredDocument[]>("list_recent_documents", { docType, limit }),
  ingestFile: (sessionId: string | null, path: string, docType: DocType) =>
    invoke<IngestedDocument>("ingest_document", { sessionId, path, docType }),
  ingestText: (sessionId: string | null, docType: DocType, name: string, text: string) =>
    invoke<IngestedDocument>("ingest_text_document", { sessionId, docType, name, text }),
  remove: (documentId: string) => invoke<boolean>("delete_document", { documentId }),
  original: (documentId: string) => invoke<DocumentOriginal>("read_document_original", { documentId }),
  contextPreview: (sessionId: string | null) => invoke<ContextPreview>("get_context_preview", { sessionId }),
  fetchJobPost: (url: string) => invoke<string>("fetch_job_post_text", { url }),
};

export const history = {
  list: () => invoke<PastSessionSummary[]>("list_past_sessions"),
  transcript: (sessionId: string) => invoke<TranscriptTurn[]>("get_session_transcript", { sessionId }),
  answers: (sessionId: string) => invoke<SessionAnswer[]>("get_session_answers", { sessionId }),
  context: (sessionId: string) => invoke<string[]>("get_session_context", { sessionId }),
  summarize: (sessionId: string) => invoke<SessionSummary>("summarize_past_session", { sessionId }),
  search: (query: string) => invoke<TranscriptSearchResult[]>("search_transcripts", { query }),
  remove: (sessionId: string) => invoke<boolean>("delete_session", { sessionId }),
};

export const speech = {
  engines: () => invoke<EngineKind[]>("list_stt_engines"),
  current: () => invoke<EngineKind | null>("get_stt_engine"),
  choose: (engine: EngineKind) => invoke<void>("set_stt_engine", { engine }),
  hasKey: (engine: EngineKind) => invoke<boolean>("has_stt_api_key", { engine }),
  saveKey: (engine: EngineKind, apiKey: string) => invoke<void>("set_stt_api_key", { engine, apiKey }),
  modelDownloaded: () => invoke<boolean>("is_parakeet_model_downloaded"),
  downloadModel: () => invoke<void>("download_parakeet_model"),
};

export const model = {
  providers: () => invoke<LlmProviderKind[]>("list_llm_providers"),
  models: (provider: LlmProviderKind) => invoke<string[]>("list_llm_models", { provider }),
  currentProvider: () => invoke<LlmProviderKind | null>("get_llm_provider"),
  /** Reaches the default AI model and speech engine with their saved keys. */
  connections: () => invoke<ModelConnections>("check_model_connections"),
  currentModel: (provider: LlmProviderKind) => invoke<string | null>("get_llm_model", { provider }),
  setProvider: (provider: LlmProviderKind) => invoke<void>("set_llm_provider", { provider }),
  setModel: (provider: LlmProviderKind, modelName: string) =>
    invoke<void>("set_llm_model", { provider, model: modelName }),
  hasKey: (provider: LlmProviderKind) => invoke<boolean>("has_llm_api_key", { provider }),
  saveKey: (provider: LlmProviderKind, apiKey: string) => invoke<void>("set_llm_api_key", { provider, apiKey }),
  tier: () => invoke<string>("get_llm_service_tier"),
  setTier: (tier: string) => invoke<void>("set_llm_service_tier", { tier }),
  rules: () => invoke<AnswerRules>("get_answer_rules"),
  saveRules: (text: string) => invoke<AnswerRules>("set_answer_rules", { text }),
  resetRules: () => invoke<AnswerRules>("reset_answer_rules"),
};

export type { Session };
