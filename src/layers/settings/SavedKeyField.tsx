import { useState, type KeyboardEvent } from "react";
import { Badge, Button, Field, TextInput } from "../../ui";
import { SettingsBullets } from "./SettingsBullets";

export interface SavedKeyFieldProps {
  label: string;
  hasKey: boolean;
  saving: boolean;
  error: string | null;
  onSave: (apiKey: string) => Promise<boolean>;
}

/**
 * Saved-key row for Settings. If a key is already stored, the box stays
 * hidden until Change. The stored key is only replaced when Enter is
 * pressed with a new value. Cancel / Escape keeps the old key.
 */
export function SavedKeyField({ label, hasKey, saving, error, onSave }: SavedKeyFieldProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  const cancel = () => {
    setDraft("");
    setEditing(false);
  };

  const commit = async () => {
    const next = draft.trim();
    if (!next || saving) return;
    const ok = await onSave(next);
    if (ok) {
      setDraft("");
      setEditing(false);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      void commit();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancel();
    }
  };

  if (hasKey && !editing) {
    return (
      <div className="settings-key">
        <span className="u-field__label">{label}</span>
        <div className="settings-key__saved">
          <Badge tone="ok">Already saved</Badge>
          <Button type="button" variant="secondary" onClick={() => setEditing(true)}>
            Change
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="settings-key settings-key--edit">
      <Field label={label}>
        <TextInput
          type="password"
          mono
          autoFocus={editing || !hasKey}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Paste key here"
        />
      </Field>
      <div className="field-row">
        {hasKey ? (
          <Button type="button" variant="secondary" onClick={cancel}>
            Cancel
          </Button>
        ) : null}
        {saving ? <Badge tone="accent">Saving</Badge> : null}
      </div>
      <SettingsBullets
        items={
          hasKey
            ? ["A new key replaces the saved one only when you press Enter.", "Cancel keeps the current key."]
            : ["Paste the key, then press Enter to save."]
        }
      />
      {error ? <p className="settings-error">Problem: {error}</p> : null}
    </div>
  );
}
