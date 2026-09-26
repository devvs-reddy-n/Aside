import type { CopilotSummary, EngineKind, LlmProviderKind } from "../bridge/types";
import type { PageId } from "./router";
import { engineName, engineNeedsKey, modelLabel } from "./labels.ts";

/** What Aside can actually verify about itself before a call starts. */
export interface SystemSnapshot {
  engine: EngineKind | null;
  speechKey: boolean;
  parakeetReady: boolean;
  provider: LlmProviderKind | null;
  model: string | null;
  aiKey: boolean;
  /** Whether Aside is hidden from screen sharing (null until known). */
  hidden: boolean | null;
}

export interface Check {
  id: "speech" | "ai" | "sources" | "privacy" | "permissions";
  status: "ok" | "todo" | "info";
  title: string;
  detail: string;
  fix?: { label: string; page: PageId; param?: string };
}

export function speechProblem(system: SystemSnapshot): string | null {
  if (!system.engine) return "No speech engine chosen yet.";
  if (engineNeedsKey(system.engine) && !system.speechKey) return `${engineName(system.engine)} needs an API key.`;
  if (system.engine === "parakeet_local" && !system.parakeetReady) return "Parakeet's one-time download isn't done yet.";
  return null;
}

export function aiProblem(system: SystemSnapshot): string | null {
  if (!system.provider || !system.model) return "No AI model chosen yet.";
  if (!system.aiKey) return "The AI model needs an API key.";
  return null;
}

/**
 * The pre-call checklist. Only things that can truly be checked from here are
 * marked done or not done. macOS permissions cannot be checked ahead of time,
 * so that row is honest information, never a green tick.
 */
export function checksFor(system: SystemSnapshot, copilot: CopilotSummary | null): Check[] {
  const speech = speechProblem(system);
  const ai = aiProblem(system);
  const checks: Check[] = [
    speech
      ? { id: "speech", status: "todo", title: "Speech recognition", detail: speech, fix: { label: "Set up", page: "settings", param: "speech" } }
      : { id: "speech", status: "ok", title: "Speech recognition", detail: `${engineName(system.engine as EngineKind)} is ready.` },
    ai
      ? { id: "ai", status: "todo", title: "AI model", detail: ai, fix: { label: "Set up", page: "settings", param: "ai" } }
      : { id: "ai", status: "ok", title: "AI model", detail: `${modelLabel(system.provider as LlmProviderKind, system.model as string)} is ready.` },
  ];
  if (copilot) {
    checks.push(
      copilot.has_resume
        ? { id: "sources", status: "ok", title: "Your background", detail: `Resume and ${copilot.document_count - 1} other source${copilot.document_count === 2 ? "" : "s"} attached.` }
        : { id: "sources", status: "info", title: "Your background", detail: "No resume attached. Answers will be more generic without one." },
    );
  }
  checks.push(
    system.hidden === null
      ? { id: "privacy", status: "info", title: "Screen sharing", detail: "Open Aside to see whether it's hidden from screen sharing." }
      : system.hidden
        ? { id: "privacy", status: "ok", title: "Screen sharing", detail: "Aside is hidden from screen sharing, screenshots and recordings." }
        : { id: "privacy", status: "info", title: "Screen sharing", detail: "Aside is visible to screen sharing right now.", fix: { label: "Change", page: "settings", param: "privacy" } },
    {
      id: "permissions",
      status: "info",
      title: "Microphone and system audio",
      detail: "macOS asks the first time. Aside can't check ahead of time. If you have said no before, allow it in System Settings → Privacy & Security.",
    },
  );
  return checks;
}

/** Things that will stop Aside from doing its job. */
export function blockers(checks: Check[]): Check[] {
  return checks.filter((check) => check.status === "todo");
}
