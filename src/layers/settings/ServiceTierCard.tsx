import { useEffect, useState } from "react";
import { Badge, Card, Field, Select } from "../../ui";
import { invoke } from "../../platform/invoke";
import { isTauriRuntime } from "../../platform/runtime";
import { SettingsBullets } from "./SettingsBullets";

const SERVICE_TIER_OPTIONS: { value: string; label: string }[] = [
  { value: "", label: "Normal" },
  { value: "flex", label: "Flex" },
  { value: "priority", label: "Priority" },
];

/**
 * OpenAI's Normal / Flex / Priority lane. Kept off the AI model card so
 * the two model pickers can sit on one row.
 */
export function ServiceTierCard() {
  const tauri = isTauriRuntime();
  const [tier, setTier] = useState("");
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    (async () => {
      const result = await invoke<string>("get_llm_service_tier");
      if (result.ok) setTier(result.value);
      else setError(result.error);
    })();
  }, []);

  return (
    <Card className="layer-page__section" title="Speed vs. cost">
      <div className="step-body">
        {tauri ? (
          <Field label="Lane">
            <Select
              value={tier}
              disabled={busy}
              onChange={async (event) => {
                const next = event.target.value;
                setBusy(true);
                setSaved(false);
                const result = await invoke<void>("set_llm_service_tier", { tier: next });
                setBusy(false);
                if (result.ok) {
                  setTier(next);
                  setSaved(true);
                } else {
                  setError(result.error);
                }
              }}
            >
              {SERVICE_TIER_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </Select>
          </Field>
        ) : null}
        {saved ? <Badge tone="ok">Saved</Badge> : null}
        <SettingsBullets
          items={[
            "Normal is regular. Flex is cheaper and may wait. Priority is fastest and costs more.",
            "OpenAI only. Saved either way.",
          ]}
        />
        {error ? <p className="settings-error">Problem: {error}</p> : null}
      </div>
    </Card>
  );
}
