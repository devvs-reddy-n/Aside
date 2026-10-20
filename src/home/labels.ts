import type { EngineKind, LlmProviderKind } from "../bridge/types";

export function providerLabel(provider: LlmProviderKind): string {
  switch (provider) {
    case "open_ai": return "OpenAI";
    case "anthropic": return "Claude";
  }
}

/** Display names only. The saved id sent to the API never changes. */
const KNOWN_MODELS: Partial<Record<LlmProviderKind, Record<string, string>>> = {
  open_ai: {
    "gpt-6-luna": "GPT-6 Luna",
    "gpt-6.1-sol": "GPT-6.1 Sol",
    "gpt-5.6-terra": "GPT-5.6 Terra",
  },
  anthropic: {
    "claude-opus-5-5": "Claude Opus 5.5",
    "claude-sonnet-5-5": "Claude Sonnet 5.5",
    "claude-haiku-5-5": "Claude Haiku 5.5",
  },
};

/** Saved as a copilot's model when it follows the default model in Settings. */
export const DEFAULT_MODEL_CHOICE = "default";

/** A copilot's model as people read it, naming what Default currently means. */
export function copilotModelLabel(provider: LlmProviderKind, model: string, fallback: { provider: LlmProviderKind | null; model: string | null }): string {
  if (model !== DEFAULT_MODEL_CHOICE) return modelLabel(provider, model);
  return fallback.provider && fallback.model ? `Default · ${modelLabel(fallback.provider, fallback.model).replace(/^[^·]+· /, "")}` : "Default";
}

/** Known names already carry the brand ("GPT-6 Luna", "Claude Opus 5.5"). */
export function modelLabel(provider: LlmProviderKind, model: string): string {
  return KNOWN_MODELS[provider]?.[model] ?? `${providerLabel(provider)} · ${model}`;
}

export const ENGINE_ORDER: EngineKind[] = ["speechmatics", "parakeet_local"];

export function engineName(engine: EngineKind): string {
  switch (engine) {
    case "speechmatics": return "Speechmatics";
    case "parakeet_local": return "Parakeet";
  }
}

export function engineNeedsKey(engine: EngineKind): boolean {
  return engine !== "parakeet_local";
}

/** ISO 639-1 codes — must match `commands::overlay::SUPPORTED_LANGUAGES`; an unknown code silently becomes English. */
export const LANGUAGES: Array<{ code: string; label: string }> = [
  { code: "en", label: "English" },
  { code: "es", label: "Spanish" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "hi", label: "Hindi" },
  { code: "pt", label: "Portuguese" },
];
