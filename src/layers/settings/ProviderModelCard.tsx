import { useCallback, useEffect, useState } from "react";
import { Badge, Card, Field, Select } from "../../ui";
import { invoke } from "../../platform/invoke";
import { isTauriRuntime } from "../../platform/runtime";
import { useLlmProviderKey } from "../../state/llmProviderKey";
import type { LlmProviderKind } from "../../platform/types";
import { SavedKeyField } from "./SavedKeyField";
import { SettingsBullets } from "./SettingsBullets";

function providerLabel(provider: LlmProviderKind): string {
  switch (provider) {
    case "open_ai":
      return "OpenAI";
    case "anthropic":
      return "Anthropic";
    case "gemini":
      return "Google Gemini";
    case "groq":
      return "Groq";
  }
}

/** Display names only. The saved id sent to the API is unchanged. */
const KNOWN_MODEL_LABELS: Partial<Record<LlmProviderKind, Record<string, string>>> = {
  open_ai: {
    "gpt-5.6-luna": "GPT-5.6 (Luna)",
    "gpt-5.6-terra": "GPT-5.6 (Terra)",
    "gpt-5.5": "GPT-5.5",
    "gpt-5.4": "GPT-5.4",
    "gpt-5.4-mini": "GPT-5.4 Mini",
  },
};

function modelLabel(provider: LlmProviderKind, model: string): string {
  const known = KNOWN_MODEL_LABELS[provider]?.[model];
  return `${providerLabel(provider)} ${known ?? model}`;
}

interface ModelOption {
  provider: LlmProviderKind;
  model: string;
}

/**
 * Which AI model writes answers, plus its key. Speed vs. cost lives in
 * its own card below so this can sit on one row with Speech.
 */
export function ProviderModelCard() {
  const tauri = isTauriRuntime();
  const [options, setOptions] = useState<ModelOption[]>([]);
  const [provider, setProvider] = useState<LlmProviderKind | null>(null);
  const [model, setModel] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const llm = useLlmProviderKey(provider ?? "open_ai");

  useEffect(() => {
    if (!isTauriRuntime()) return;
    (async () => {
      const list = await invoke<LlmProviderKind[]>("list_llm_providers");
      if (!list.ok) {
        setError(list.error);
        return;
      }
      const perProvider = await Promise.all(
        list.value.map(async (kind) => {
          const models = await invoke<string[]>("list_llm_models", { provider: kind });
          return models.ok ? models.value.map((m) => ({ provider: kind, model: m })) : [];
        }),
      );
      setOptions(perProvider.flat());

      const currentProvider = await invoke<LlmProviderKind | null>("get_llm_provider");
      const active = currentProvider.ok && currentProvider.value ? currentProvider.value : list.value[0] ?? null;
      setProvider(active);
      if (active) {
        const current = await invoke<string | null>("get_llm_model", { provider: active });
        if (current.ok) setModel(current.value);
        else setError(current.error);
      }
    })();
  }, []);

  const chooseModel = useCallback(async (next: ModelOption) => {
    setBusy(true);
    setError(null);
    setSaved(false);
    const providerResult = await invoke<void>("set_llm_provider", { provider: next.provider });
    if (!providerResult.ok) {
      setBusy(false);
      setError(providerResult.error);
      return;
    }
    const modelResult = await invoke<void>("set_llm_model", { provider: next.provider, model: next.model });
    setBusy(false);
    if (!modelResult.ok) {
      setError(modelResult.error);
      return;
    }
    setProvider(next.provider);
    setModel(next.model);
    setSaved(true);
  }, []);

  return (
    <Card className="layer-page__section" title="AI model">
      <div className="step-body">
        {!tauri ? (
          <SettingsBullets items={["Open Aside.app to change this."]} />
        ) : (
          <>
            {options.length > 0 ? (
              <Field label="Model">
                <Select
                  value={model ?? ""}
                  disabled={busy}
                  onChange={(event) => {
                    const next = options.find((o) => o.model === event.target.value);
                    if (next) void chooseModel(next);
                  }}
                >
                  <option value="" disabled>
                    Choose one
                  </option>
                  {options.map((option) => (
                    <option key={`${option.provider}:${option.model}`} value={option.model}>
                      {modelLabel(option.provider, option.model)}
                    </option>
                  ))}
                </Select>
              </Field>
            ) : null}

            {saved ? <Badge tone="ok">Saved</Badge> : null}

            <SavedKeyField
              label="API key"
              hasKey={llm.hasKey}
              saving={llm.saving}
              error={llm.error}
              onSave={(apiKey) => llm.saveKey(apiKey)}
            />

            <SettingsBullets items={["This model writes spoken, typed, and chat answers."]} />
          </>
        )}

        {error ? <p className="settings-error">Problem: {error}</p> : null}
      </div>
    </Card>
  );
}
