import { useState } from "react";
import { Badge, Button, Card, Field, Select } from "../../ui";
import { engineNeedsKey, type SttEngineState } from "../../state/sttEngine";
import type { EngineKind } from "../../platform/types";
import { SavedKeyField } from "./SavedKeyField";
import { SettingsBullets } from "./SettingsBullets";

export interface SpeechEngineCardProps {
  stt: SttEngineState;
}

function shortEngineLabel(engine: EngineKind): string {
  switch (engine) {
    case "speechmatics":
      return "Speechmatics";
    case "deepgram":
      return "Deepgram";
    case "parakeet_local":
      return "Parakeet";
  }
}

/**
 * Which service turns speech into text. Sits beside the AI model picker.
 * Device choice lives in the live overlay toolbar.
 */
export function SpeechEngineCard({ stt }: SpeechEngineCardProps) {
  const [savingKey, setSavingKey] = useState(false);
  const ready =
    stt.engine != null &&
    (!engineNeedsKey(stt.engine) || stt.hasKey) &&
    (stt.engine !== "parakeet_local" || stt.modelDownloaded);

  return (
    <Card
      className="layer-page__section"
      title="Speech"
      footer={
        <Badge tone={!stt.tauri ? "bad" : ready ? "ok" : "warn"}>
          {!stt.tauri ? "Not available" : ready ? "Ready" : "Needs attention"}
        </Badge>
      }
    >
      <div className="step-body">
        {!stt.tauri ? (
          <SettingsBullets items={["Open Aside.app to change this."]} />
        ) : (
          <>
            <Field label="Engine">
              <Select
                value={stt.engine ?? ""}
                onChange={(event) => {
                  const next = event.target.value;
                  if (next) void stt.chooseEngine(next as EngineKind);
                }}
              >
                <option value="" disabled>
                  Choose one
                </option>
                {stt.engines.map((engine) => (
                  <option key={engine} value={engine}>
                    {shortEngineLabel(engine)}
                  </option>
                ))}
              </Select>
            </Field>

            {stt.engine && engineNeedsKey(stt.engine) ? (
              <SavedKeyField
                label="API key"
                hasKey={stt.hasKey}
                saving={savingKey}
                error={stt.error}
                onSave={async (apiKey) => {
                  setSavingKey(true);
                  const ok = await stt.saveKey(apiKey);
                  setSavingKey(false);
                  return ok;
                }}
              />
            ) : null}

            {stt.engine === "parakeet_local" ? (
              <div className="field-row">
                <Button
                  variant="primary"
                  disabled={stt.downloading || stt.modelDownloaded}
                  onClick={() => void stt.downloadModel()}
                >
                  {stt.modelDownloaded ? "Downloaded" : stt.downloading ? "Downloading" : "Download"}
                </Button>
                <Badge tone={stt.modelDownloaded ? "ok" : stt.downloading ? "accent" : "warn"}>
                  {stt.modelDownloaded ? "Ready" : stt.downloading ? "Working" : "Needed"}
                </Badge>
              </div>
            ) : null}

            <SettingsBullets items={["Speechmatics needs a key. Parakeet runs on this computer after one download."]} />

            {stt.error && !(stt.engine && engineNeedsKey(stt.engine)) ? (
              <p className="settings-error">Problem: {stt.error}</p>
            ) : null}
          </>
        )}
      </div>
    </Card>
  );
}
