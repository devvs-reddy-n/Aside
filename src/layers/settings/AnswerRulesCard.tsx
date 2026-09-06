import { useCallback, useState } from "react";
import { Badge, Button, Card, Dialog, TextArea } from "../../ui";
import { invoke } from "../../platform/invoke";
import type { AnswerRules } from "../../platform/types";
import { SettingsBullets } from "./SettingsBullets";
import { SettingsFullWindow } from "./SettingsFullWindow";

/**
 * Strict answer rules. Edit opens a full-window warning, then a
 * full-window editor. Closing without Save leaves the stored rules unchanged.
 */
export function AnswerRulesCard() {
  const [open, setOpen] = useState(false);
  const [step, setStep] = useState<"warning" | "editor">("warning");
  const [loaded, setLoaded] = useState(false);
  const [rules, setRules] = useState<AnswerRules | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);

  const load = useCallback(async () => {
    setBusy(true);
    setError(null);
    const result = await invoke<AnswerRules>("get_answer_rules");
    setBusy(false);
    if (result.ok) {
      setRules(result.value);
      setDraft(result.value.text);
      setLoaded(true);
    } else {
      setError(result.error);
    }
  }, []);

  const closeWindow = () => {
    setOpen(false);
    setStep("warning");
    setLoaded(false);
    setRules(null);
    setDraft("");
    setSaved(false);
    setError(null);
    setConfirmReset(false);
  };

  const requestOpen = () => {
    setStep("warning");
    setOpen(true);
  };

  const continuePastWarning = () => {
    setStep("editor");
    if (!loaded) void load();
  };

  const save = useCallback(async () => {
    setBusy(true);
    setError(null);
    setSaved(false);
    const result = await invoke<AnswerRules>("set_answer_rules", { text: draft });
    setBusy(false);
    if (result.ok) {
      setRules(result.value);
      setDraft(result.value.text);
      setSaved(true);
    } else {
      setError(result.error);
    }
  }, [draft]);

  const doReset = useCallback(async () => {
    setBusy(true);
    setError(null);
    const result = await invoke<AnswerRules>("reset_answer_rules");
    setBusy(false);
    setConfirmReset(false);
    if (result.ok) {
      setRules(result.value);
      setDraft(result.value.text);
      setSaved(true);
    } else {
      setError(result.error);
    }
  }, []);

  return (
    <Card className="layer-page__section" title="Answer rules">
      <div className="step-body">
        <Button variant="secondary" onClick={requestOpen}>
          Edit
        </Button>
        <SettingsBullets items={["These rules shape every answer from now on."]} />
      </div>

      <SettingsFullWindow
        open={open && step === "warning"}
        title="Edit answer rules?"
        onBack={closeWindow}
        onClose={closeWindow}
        footer={
          <>
            <Button variant="secondary" onClick={closeWindow}>
              Exit
            </Button>
            <Button variant="primary" onClick={continuePastWarning}>
              Continue
            </Button>
          </>
        }
      >
        <SettingsBullets
          items={[
            "A change here affects every future answer.",
            "Only continue if you mean to change how answers are written.",
          ]}
        />
      </SettingsFullWindow>

      <SettingsFullWindow
        open={open && step === "editor"}
        title="Answer rules"
        onBack={closeWindow}
        onClose={closeWindow}
        footer={
          <>
            <Button variant="secondary" disabled={busy || !rules || rules.isDefault} onClick={() => setConfirmReset(true)}>
              Reset
            </Button>
            <Button variant="secondary" onClick={closeWindow}>
              Close
            </Button>
            <Button variant="primary" disabled={busy || draft.trim().length === 0} onClick={() => void save()}>
              {busy ? "Saving" : "Save"}
            </Button>
          </>
        }
      >
        <div className="settings-rules-editor">
          {busy && !loaded ? (
            <p className="settings-error">Loading.</p>
          ) : rules ? (
            <>
              <TextArea
                mono
                showCount
                value={draft}
                onChange={(event) => {
                  setDraft(event.target.value);
                  setSaved(false);
                }}
              />
              <div className="settings-rules-editor__meta">
                {saved ? <Badge tone="ok">Saved</Badge> : null}
                {rules.isDefault ? <Badge tone="neutral">Original text</Badge> : null}
                {!rules.meetsCacheFloor ? (
                  <span className="settings-error">Short rules can lose the cheaper repeat rate.</span>
                ) : null}
              </div>
              {error ? <p className="settings-error">Problem: {error}</p> : null}
            </>
          ) : (
            <p className="settings-error">Problem: {error ?? "Could not load the answer rules."}</p>
          )}
        </div>
      </SettingsFullWindow>

      <Dialog
        open={confirmReset}
        onClose={() => setConfirmReset(false)}
        title="Reset the answer rules?"
        destructive
        footer={
          <>
            <Button variant="secondary" onClick={() => setConfirmReset(false)}>
              Cancel
            </Button>
            <Button variant="destructive" disabled={busy} onClick={() => void doReset()}>
              {busy ? "Resetting" : "Reset"}
            </Button>
          </>
        }
      >
        <SettingsBullets items={["This replaces the editor with the original rules. This cannot be undone."]} />
      </Dialog>
    </Card>
  );
}
