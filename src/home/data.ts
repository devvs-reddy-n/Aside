import { open } from "@tauri-apps/plugin-dialog";
import * as api from "../bridge/api";
import type { Result } from "../bridge/api";
import { onEvent } from "../bridge/events";
import { SESSION_EVENTS, STEALTH_CHANGED_EVENT } from "../bridge/eventNames";
import { backendAvailable, isDemoMode } from "../bridge/runtime";
import type {
  ActiveSessionInfo,
  ChannelId,
  Copilot,
  CopilotInput,
  CopilotSummary,
  DocType,
  EngineKind,
  IngestedDocument,
  ModelConnections,
  PastSessionSummary,
  Session,
  StoredDocument,
} from "../bridge/types";
import { createStore, type Store } from "../core/store";
import type { SystemSnapshot } from "./readiness.ts";

/* ================================ system ================================ */

export interface SystemState extends SystemSnapshot {
  loaded: boolean;
  engines: EngineKind[];
  /** The call running right now, if any. */
  active: ActiveSessionInfo | null;
  /** Whether the default AI model and speech engine were reached (null while checking). */
  links: ModelConnections | null;
}

export const systemStore = createStore<SystemState>({
  loaded: false,
  engines: [],
  engine: null,
  speechKey: false,
  parakeetReady: false,
  provider: null,
  model: null,
  aiKey: false,
  hidden: null,
  active: null,
  links: null,
});

const RECHECK_MS = 60_000;
let lastCheck = { at: 0, setup: "" };

/** Reaches both default models. Runs again when the setup changes, otherwise at most once a minute. */
async function checkConnections(): Promise<void> {
  const state = systemStore.get();
  const setup = [state.engine, state.speechKey, state.parakeetReady, state.provider, state.model, state.aiKey].join("|");
  const changed = setup !== lastCheck.setup;
  if (!changed && Date.now() - lastCheck.at < RECHECK_MS) return;
  lastCheck = { at: Date.now(), setup };
  // A changed setup shows "checking" again; a routine recheck keeps the last dots until it finishes.
  if (changed) systemStore.set({ links: null });
  const result = await api.model.connections();
  systemStore.set({ links: result.ok ? result.value : { ai: { state: "unchecked", detail: result.error }, speech: { state: "unchecked", detail: result.error } } });
}

export async function refreshSystem(): Promise<void> {
  if (!backendAvailable()) {
    systemStore.set({ loaded: true });
    return;
  }
  const [engines, engine, parakeet, provider, hidden, active] = await Promise.all([
    api.speech.engines(),
    api.speech.current(),
    api.speech.modelDownloaded(),
    api.model.currentProvider(),
    api.stealth.get(),
    api.live.activeSession(),
  ]);
  const engineValue = engine.ok ? engine.value : null;
  const providerValue = provider.ok ? provider.value : null;
  const [speechKey, aiKey, model] = await Promise.all([
    engineValue && engineValue !== "parakeet_local" ? api.speech.hasKey(engineValue) : Promise.resolve({ ok: true as const, value: false }),
    providerValue ? api.model.hasKey(providerValue) : Promise.resolve({ ok: true as const, value: false }),
    providerValue ? api.model.currentModel(providerValue) : Promise.resolve({ ok: true as const, value: null }),
  ]);
  systemStore.set({
    loaded: true,
    engines: engines.ok ? engines.value : [],
    engine: engineValue,
    speechKey: speechKey.ok ? speechKey.value : false,
    parakeetReady: parakeet.ok ? parakeet.value : false,
    provider: providerValue,
    model: model.ok ? model.value : null,
    aiKey: aiKey.ok ? aiKey.value : false,
    hidden: hidden.ok ? hidden.value : null,
    active: active.ok ? active.value : null,
  });
  void checkConnections();
}

/** Keeps the system snapshot in step with the backend for the life of the window. */
export function watchSystem(onCallEnded: () => void): () => void {
  const stops = [
    onEvent<ActiveSessionInfo>(SESSION_EVENTS.started, (info) => systemStore.set({ active: info })),
    onEvent<null>(SESSION_EVENTS.ended, () => {
      systemStore.set({ active: null });
      onCallEnded();
    }),
    onEvent<boolean>(STEALTH_CHANGED_EVENT, (hidden) => systemStore.set({ hidden })),
  ];
  return () => stops.forEach((stop) => stop());
}

/* =============================== copilots =============================== */

export const copilotsStore = createStore<{ items: CopilotSummary[]; loaded: boolean; error: string | null; hiddenIds: string[] }>({
  items: [],
  loaded: false,
  error: null,
  hiddenIds: [],
});

export async function loadCopilots(): Promise<void> {
  if (!backendAvailable()) {
    copilotsStore.set({ loaded: true });
    return;
  }
  const result = await api.copilots.list();
  if (result.ok) copilotsStore.set({ items: result.value, loaded: true, error: null });
  else copilotsStore.set({ loaded: true, error: result.error });
}

export async function saveCopilot(id: string | null, input: CopilotInput): Promise<Result<Copilot>> {
  const result = id ? await api.copilots.update(id, input) : await api.copilots.create(input);
  if (result.ok) await loadCopilots();
  return result;
}

/** Hides (or un-hides) one item on screen while a delete waits for its Undo window to pass. */
export function setHidden<S extends { hiddenIds: string[] }>(store: Store<S>, id: string, hidden: boolean): void {
  store.update((state) => ({
    ...state,
    hiddenIds: hidden ? [...state.hiddenIds, id] : state.hiddenIds.filter((entry) => entry !== id),
  }));
}

/* ================================ library =============================== */

export const libraryStore = createStore<{ docs: StoredDocument[]; loaded: boolean; error: string | null; hiddenIds: string[] }>({
  docs: [],
  loaded: false,
  error: null,
  hiddenIds: [],
});

const LIBRARY_LIMIT = 200;

export async function loadLibrary(): Promise<void> {
  if (!backendAvailable()) {
    libraryStore.set({ loaded: true });
    return;
  }
  const types = await api.documents.types();
  const kinds: DocType[] = types.ok && types.value.length > 0 ? types.value : ["resume", "job_description", "other"];
  const lists = await Promise.all(kinds.map((kind) => api.documents.recent(kind, LIBRARY_LIMIT)));
  const docs: StoredDocument[] = [];
  let error: string | null = null;
  for (const list of lists) {
    if (list.ok) docs.push(...list.value);
    else error = error ?? list.error;
  }
  docs.sort((a, b) => b.created_at_ms - a.created_at_ms);
  libraryStore.set({ docs, loaded: true, error });
}

/** Opens the Mac's file picker. Returns the chosen paths (empty if cancelled). */
export async function pickDocumentFiles(): Promise<string[]> {
  if (isDemoMode()) return ["/Users/demo/Documents/Product-Roadmap-Notes.pdf"];
  if (!backendAvailable()) return [];
  const chosen = await open({
    title: "Choose a resume or document",
    multiple: true,
    filters: [{ name: "PDF or Word document", extensions: ["pdf", "docx"] }],
  });
  if (!chosen) return [];
  return Array.isArray(chosen) ? chosen : [chosen];
}

export interface IngestReport {
  added: IngestedDocument[];
  failed: Array<{ path: string; error: string }>;
}

export async function ingestPaths(paths: string[], docType: DocType): Promise<IngestReport> {
  const report: IngestReport = { added: [], failed: [] };
  for (const path of paths) {
    if (!/\.(pdf|docx)$/i.test(path)) {
      report.failed.push({ path, error: "Only PDF and Word (.docx) files can be added." });
      continue;
    }
    const result = await api.documents.ingestFile(null, path, docType);
    if (result.ok) report.added.push(result.value);
    else report.failed.push({ path, error: result.error });
  }
  if (report.added.length > 0) await loadLibrary();
  return report;
}

export async function pasteDocument(docType: DocType, name: string, text: string): Promise<Result<IngestedDocument>> {
  const result = await api.documents.ingestText(null, docType, name, text);
  if (result.ok) await loadLibrary();
  return result;
}

/* ================================ history =============================== */

export const historyStore = createStore<{ sessions: PastSessionSummary[]; loaded: boolean; error: string | null; hiddenIds: string[] }>({
  sessions: [],
  loaded: false,
  error: null,
  hiddenIds: [],
});

export async function loadHistory(): Promise<void> {
  if (!backendAvailable()) {
    historyStore.set({ loaded: true });
    return;
  }
  const result = await api.history.list();
  if (result.ok) historyStore.set({ sessions: result.value, loaded: true, error: null });
  else historyStore.set({ loaded: true, error: result.error });
}

/* ============================== starting a call ============================ */

export interface ChannelFailure {
  channel: ChannelId;
  message: string;
}

export interface PreparedCall {
  session: Session;
  /** Ears that could not be started. The call can still go ahead without them. */
  failures: ChannelFailure[];
}

/** The stages of starting a call, in order, so the screen can show where it is. */
export type LaunchStep = "session" | "mic" | "system" | "window";

export interface LaunchOptions {
  onStep?: (step: LaunchStep) => void;
}

/**
 * Creates the call from a Copilot and starts listening on both ears (each can be turned off in the live window).
 * Nothing is shown yet: the caller decides whether to go live (or offer a fix first).
 */
export async function prepareCall(copilotId: string, options: LaunchOptions = {}): Promise<Result<PreparedCall>> {
  options.onStep?.("session");
  const started = await api.copilots.startCall(copilotId);
  if (!started.ok) return started;
  const session = started.value.session;
  const failures: ChannelFailure[] = [];
  for (const channel of ["mic", "system"] as ChannelId[]) {
    options.onStep?.(channel);
    const pinned = await api.audio.pinnedDevice(channel);
    const capture = await api.audio.start(channel, pinned.ok ? pinned.value : null);
    if (!capture.ok) {
      failures.push({ channel, message: capture.error });
      continue;
    }
    const transcription = await api.audio.startTranscription(channel, session.save_transcript ? session.id : null);
    if (!transcription.ok) {
      await api.audio.stop(channel);
      failures.push({ channel, message: transcription.error });
    }
  }
  return { ok: true, value: { session, failures } };
}

/** Shows the live window and steps this one out of the way. */
export async function goLive(): Promise<Result<void>> {
  const opened = await api.windows.openOverlay();
  if (!opened.ok) return opened;
  await api.windows.hideMain();
  return { ok: true, value: undefined };
}

/** Puts back the live window for a call that is already running. */
export async function returnToCall(): Promise<Result<void>> {
  return goLive();
}
