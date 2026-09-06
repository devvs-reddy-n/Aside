import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../platform/invoke";
import { isTauriRuntime } from "../platform/runtime";
import type { EngineKind } from "../platform/types";

/** Display order: cloud engine first, local fallback last — matches the
 *  order the working diagnostic build used. */
export const ENGINE_ORDER: EngineKind[] = ["speechmatics", "parakeet_local"];

export function engineLabel(engine: EngineKind): string {
  switch (engine) {
    case "speechmatics":
      return "Speechmatics (cloud, needs a key)";
    case "deepgram":
      return "Deepgram (cloud, needs a key)";
    case "parakeet_local":
      return "Parakeet (runs on this computer, free, one large download)";
  }
}

/** Only the local Parakeet engine needs no key — every cloud engine does. */
export function engineNeedsKey(engine: EngineKind): boolean {
  return engine !== "parakeet_local";
}

export interface SttEngineState {
  tauri: boolean;
  /** The engines Aside currently offers a choice between. */
  engines: EngineKind[];
  /** The engine saved in settings, or `null` before anything has been
   *  chosen yet. */
  engine: EngineKind | null;
  /** Whether a key is saved for the *currently chosen* engine. Always
   *  `true` for Parakeet, which needs no key. */
  hasKey: boolean;
  /** Whether the one local model Aside uses has finished downloading. */
  modelDownloaded: boolean;
  /** True while `download_parakeet_model` is running. The backend does
   *  not report progress as it downloads (see this hook's module doc
   *  comment) so this can only be a "working on it" state, not a
   *  percentage. */
  downloading: boolean;
  error: string | null;
  /** Picks an engine and saves it. If it needs a key that is not saved
   *  yet, `hasKey` becomes `false` so the caller can prompt for one. */
  chooseEngine: (engine: EngineKind) => Promise<void>;
  /** Saves an API key for whichever engine is currently chosen. */
  saveKey: (apiKey: string) => Promise<boolean>;
  /** Starts the one-time Parakeet download. Resolves once it either
   *  finishes or fails — there is nothing to poll in between. */
  downloadModel: () => Promise<void>;
  refresh: () => Promise<void>;
}

/**
 * Which speech-to-text engine is selected, whether it has what it needs
 * to run (a saved key, or a downloaded model), and the actions to fix
 * whichever is missing.
 *
 * **Download progress, verified against the Rust source:**
 * `commands::stt::download_parakeet_model` (`src-tauri/src/commands/stt.rs`)
 * is a single `async` command that only resolves when the whole ~480 MB
 * download either finishes or fails — it emits no progress event as it
 * goes. This hook's `downloading` flag can therefore only mean "the
 * download is in progress," never "40% done." Any screen that wants a
 * percentage bar would need a new Rust command first; report this as a
 * gap rather than inventing a fake progress number.
 */
export function useSttEngine(): SttEngineState {
  const tauri = isTauriRuntime();
  const [engines, setEngines] = useState<EngineKind[]>([]);
  const [engine, setEngine] = useState<EngineKind | null>(null);
  const [hasKey, setHasKey] = useState(false);
  const [modelDownloaded, setModelDownloaded] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const engineRef = useRef<EngineKind | null>(null);
  engineRef.current = engine;

  const checkKey = useCallback(async (forEngine: EngineKind) => {
    if (!engineNeedsKey(forEngine)) {
      setHasKey(true);
      return;
    }
    const result = await invoke<boolean>("has_stt_api_key", { engine: forEngine });
    if (result.ok) setHasKey(result.value);
    else setError(result.error);
  }, []);

  const refresh = useCallback(async () => {
    if (!isTauriRuntime()) return;
    const listed = await invoke<EngineKind[]>("list_stt_engines");
    if (listed.ok) setEngines(listed.value.length > 0 ? listed.value : ENGINE_ORDER);
    else setError(listed.error);

    const current = await invoke<EngineKind | null>("get_stt_engine");
    if (current.ok) {
      setEngine(current.value);
      if (current.value) await checkKey(current.value);
    } else {
      setError(current.error);
    }

    const model = await invoke<boolean>("is_parakeet_model_downloaded");
    if (model.ok) setModelDownloaded(model.value);
    else setError(model.error);
  }, [checkKey]);

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const chooseEngine = useCallback(
    async (next: EngineKind) => {
      setError(null);
      const saved = await invoke<void>("set_stt_engine", { engine: next });
      if (!saved.ok) {
        setError(saved.error);
        return;
      }
      setEngine(next);
      await checkKey(next);
    },
    [checkKey],
  );

  const saveKey = useCallback(async (apiKey: string) => {
    const current = engineRef.current;
    if (!current) {
      setError("Choose a speech engine before saving a key.");
      return false;
    }
    setError(null);
    const saved = await invoke<void>("set_stt_api_key", { engine: current, apiKey });
    if (!saved.ok) {
      setError(saved.error);
      return false;
    }
    setHasKey(true);
    return true;
  }, []);

  const downloadModel = useCallback(async () => {
    setDownloading(true);
    setError(null);
    const result = await invoke<void>("download_parakeet_model");
    if (result.ok) {
      const ready = await invoke<boolean>("is_parakeet_model_downloaded");
      if (ready.ok) setModelDownloaded(ready.value);
    } else {
      setError(result.error);
    }
    setDownloading(false);
  }, []);

  return {
    tauri,
    engines: engines.length > 0 ? engines : ENGINE_ORDER,
    engine,
    hasKey,
    modelDownloaded,
    downloading,
    error,
    chooseEngine,
    saveKey,
    downloadModel,
    refresh,
  };
}
