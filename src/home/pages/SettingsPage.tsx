import { useCallback, useEffect, useState, type ReactNode } from "react";
import * as api from "../../bridge/api";
import { backendAvailable } from "../../bridge/runtime";
import type { AnswerRules, EngineKind, LlmProviderKind, OverlayPrefs } from "../../bridge/types";
import { useStore } from "../../core/store";
import { Badge, Callout } from "../../kit/Bits";
import { Button } from "../../kit/Button";
import { Dialog } from "../../kit/Dialog";
import { Segmented, Select, TextInput, Toggle } from "../../kit/Form";
import { Icon } from "../../kit/Icon";
import { Keys } from "../../kit/Keycap";
import { RichText } from "../../kit/RichText";
import { Sheet } from "../../kit/Sheet";
import { Skeleton } from "../../kit/Skeleton";
import { TextEditor } from "../../kit/TextEditor";
import { useAutosave } from "../../kit/useAutosave";
import { wordCount } from "../../core/text";
import { toast } from "../../kit/toast";
import { refreshSystem, systemStore } from "../data";
import { ENGINE_ORDER, LANGUAGES, engineBlurb, engineName, engineNeedsKey, modelLabel, providerLabel } from "../labels";
import { useTheme, type ThemePreference } from "../../theme/theme";

const SECTIONS = ["speech", "ai", "rules", "appearance", "shortcuts", "general"] as const;

/** Everything on one page, one column: a title above each card, rows of label left, control right. */
export function SettingsPage({ section }: { section: string | null }) {
  useEffect(() => {
    if (!section) return;
    const target = section === "privacy" ? "general" : (SECTIONS as readonly string[]).includes(section) ? section : null;
    if (target) document.getElementById(`st-${target}`)?.scrollIntoView({ block: "start" });
  }, [section]);

  return (
    <div className="hm-page set">
      <header className="pg-head"><h1>Settings</h1></header>
      {!backendAvailable() ? <Callout tone="warn">Open Aside.app to change settings.</Callout> : null}
      <SpeechSection />
      <AiSection />
      <RulesSection />
      <Section id="appearance" title="Appearance"><ThemeRow /></Section>
      <ShortcutsSection />
      <Section id="general" title="General"><PrivacyRow /></Section>
    </div>
  );
}

/** A title above a card. The badge only appears when something needs doing. */
function Section({ id, title, badge, children }: { id: string; title: string; badge?: ReactNode; children: ReactNode }) {
  return (
    <section className="set-sec" id={`st-${id}`} aria-labelledby={`st-${id}-title`}>
      <h2 className="set-sec__title" id={`st-${id}-title`}>{title}{badge}</h2>
      <div className="set-card">{children}</div>
    </section>
  );
}

/** One line: what it is (and a few words of help) on the left, its control on the right. */
function Row({ label, hint, children }: { label: string; hint?: ReactNode; children?: ReactNode }) {
  return (
    <div className="set-row">
      <div className="set-row__label"><b>{label}</b>{hint ? <small>{hint}</small> : null}</div>
      {children ? <div className="set-row__control">{children}</div> : null}
    </div>
  );
}

/** A saved key is never shown again. It can only be replaced, and cancelling keeps the old one. */
function KeyRow({ label, hasKey, onSave }: { label: string; hasKey: boolean; onSave: (key: string) => Promise<boolean> }) {
  const [replacing, setReplacing] = useState(false);
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);

  async function save() {
    if (!value.trim()) return;
    setBusy(true);
    setError(false);
    const ok = await onSave(value.trim());
    setBusy(false);
    if (ok) {
      setValue("");
      setReplacing(false);
      toast("Key saved", { tone: "ok" });
    } else setError(true);
  }

  if (hasKey && !replacing) {
    return (
      <Row label={label}>
        <span className="set-saved"><Icon name="check" size={13} />Saved</span>
        <Button size="sm" variant="secondary" onClick={() => setReplacing(true)}>Replace</Button>
      </Row>
    );
  }
  return (
    <div className="set-row set-row--stack">
      <div className="set-row__label"><b>{label}</b>{error ? <small className="set-bad">Couldn't save that key.</small> : <small>Kept on this Mac, never shown again.</small>}</div>
      <div className="set-inline">
        <TextInput type="password" autoComplete="off" spellCheck={false} value={value} placeholder="Paste key" aria-label={label} onChange={(event) => setValue(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void save(); }} />
        <Button variant="primary" busy={busy} disabled={!value.trim()} onClick={() => void save()}>Save</Button>
        {hasKey ? <Button variant="ghost" onClick={() => { setReplacing(false); setValue(""); setError(false); }}>Cancel</Button> : null}
      </div>
    </div>
  );
}

/* --------------------------------- speech --------------------------------- */

function SpeechSection() {
  const system = useStore(systemStore, (state) => state);
  const [downloading, setDownloading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [prefs, setPrefs] = useState<OverlayPrefs | null>(null);
  const engines = (system.engines.length > 0 ? system.engines : ENGINE_ORDER).filter((engine) => engine !== "deepgram");

  useEffect(() => {
    void api.windows.overlayPrefs().then((result) => result.ok && setPrefs(result.value));
  }, []);

  async function choose(engine: EngineKind) {
    setError(null);
    const result = await api.speech.choose(engine);
    if (!result.ok) setError(result.error);
    await refreshSystem();
  }

  async function download() {
    setDownloading(true);
    setError(null);
    const result = await api.speech.downloadModel();
    setDownloading(false);
    if (!result.ok) setError(result.error);
    await refreshSystem();
  }

  async function setLanguage(language: string) {
    if (!prefs) return;
    const next = { ...prefs, language };
    setPrefs(next);
    const saved = await api.windows.saveOverlayPrefs(next);
    if (saved.ok) setPrefs(saved.value);
  }

  const ready = system.engine === "parakeet_local" ? system.parakeetReady : system.speechKey;
  const local = system.engine === "parakeet_local";
  return (
    <Section id="speech" title="Speech" badge={system.engine && !ready ? <Badge tone="warn">{local ? "Needs download" : "Needs key"}</Badge> : null}>
      <Row label="Engine" hint={engineBlurb((system.engine ?? engines[0]) as EngineKind)}>
        <Segmented<EngineKind> label="Speech engine" value={(system.engine ?? engines[0]) as EngineKind} onChange={(engine) => void choose(engine)} options={engines.map((engine) => ({ value: engine, label: engineName(engine) }))} />
      </Row>

      {system.engine && engineNeedsKey(system.engine) ? (
        <KeyRow
          label="API key"
          hasKey={system.speechKey}
          onSave={async (key) => {
            const result = await api.speech.saveKey(system.engine as EngineKind, key);
            await refreshSystem();
            return result.ok;
          }}
        />
      ) : null}

      {local && !system.parakeetReady ? (
        <Row label="Speech model" hint="One-time download, about 480 MB.">
          <Button size="sm" variant="primary" busy={downloading} onClick={() => void download()}>{downloading ? "Downloading" : "Download"}</Button>
        </Row>
      ) : null}

      {!local ? (
        <Row label="Language" hint="What people speak on your calls.">
          <Select aria-label="Spoken language" value={prefs?.language ?? "en"} disabled={!prefs} onChange={(event) => void setLanguage(event.target.value)}>
            {LANGUAGES.map((language) => <option key={language.code} value={language.code}>{language.label}</option>)}
          </Select>
        </Row>
      ) : null}
      {error ? <p className="set-row set-bad" role="alert">{error}</p> : null}
    </Section>
  );
}

/* ---------------------------------- AI model ---------------------------------- */

interface ModelOption { provider: LlmProviderKind; model: string }

function AiSection() {
  const system = useStore(systemStore, (state) => state);
  const [options, setOptions] = useState<ModelOption[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    void (async () => {
      const providers = await api.model.providers();
      if (!providers.ok) return setError(providers.error);
      const lists = await Promise.all(providers.value.map(async (provider) => {
        const models = await api.model.models(provider);
        return models.ok ? models.value.map((model) => ({ provider, model })) : [];
      }));
      // The old "Flex / Priority" speed-vs-cost choice was removed (a few cents per call either way);
      // anyone who had picked one goes back to the regular lane.
      const tier = await api.model.tier();
      if (tier.ok && tier.value) await api.model.setTier("");
      if (alive) setOptions(lists.flat());
    })();
    return () => {
      alive = false;
    };
  }, []);

  async function chooseModel(option: ModelOption) {
    setBusy(true);
    setError(null);
    const provider = await api.model.setProvider(option.provider);
    const model = provider.ok ? await api.model.setModel(option.provider, option.model) : provider;
    setBusy(false);
    if (!model.ok) setError(model.error);
    await refreshSystem();
  }

  const provider = system.provider ?? "open_ai";
  return (
    <Section id="ai" title="AI model" badge={system.model && !system.aiKey ? <Badge tone="warn">Needs key</Badge> : null}>
      <Row label="Default model" hint="New copilots start with this. Each copilot can pick its own.">
        <Select aria-label="AI model" disabled={busy || options.length === 0} value={system.model ? `${provider}|${system.model}` : ""} onChange={(event) => { const [p, ...rest] = event.target.value.split("|"); void chooseModel({ provider: p as LlmProviderKind, model: rest.join("|") }); }}>
          <option value="" disabled>Choose one</option>
          {options.map((option) => <option key={`${option.provider}|${option.model}`} value={`${option.provider}|${option.model}`}>{modelLabel(option.provider, option.model)}</option>)}
        </Select>
      </Row>
      <KeyRow
        label={`${providerLabel(provider)} key`}
        hasKey={system.aiKey}
        onSave={async (key) => {
          const result = await api.model.saveKey(provider, key);
          await refreshSystem();
          return result.ok;
        }}
      />
      {error ? <p className="set-row set-bad" role="alert">{error}</p> : null}
    </Section>
  );
}

/* --------------------------------- privacy --------------------------------- */

function PrivacyRow() {
  const hidden = useStore(systemStore, (state) => state.hidden);
  const [pending, setPending] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);

  async function confirm() {
    if (pending === null) return;
    setBusy(true);
    // The backend saves the choice and reopens Aside so it applies from the first frame.
    const result = await api.stealth.set(pending);
    setBusy(false);
    setPending(null);
    if (!result.ok) toast(result.error, { tone: "bad" });
  }

  return (
    <>
      <Row label="Hide from screen sharing" hint="Also hides it from recordings and the Dock.">
        <Toggle checked={hidden ?? true} label="Hide from screen sharing" disabled={hidden === null} onChange={(next) => setPending(next)} />
      </Row>
      <Dialog
        open={pending !== null}
        onClose={() => setPending(null)}
        size="sm"
        title={pending ? "Turn on hiding?" : "Turn off hiding?"}
        footer={
          <>
            <Button variant="ghost" onClick={() => setPending(null)}>Cancel</Button>
            <Button variant="primary" busy={busy} onClick={() => void confirm()}>{pending ? "Turn on" : "Turn off"}</Button>
          </>
        }
      >
        <p>{pending ? "People you share your screen with won't see Aside." : "Aside will show up in screen shares and recordings."} Aside closes and reopens to apply this.</p>
      </Dialog>
    </>
  );
}

/* ------------------------------- appearance ------------------------------- */

function ThemeRow() {
  const { preference, setPreference } = useTheme();
  return (
    <Row label="Theme">
      <Segmented<ThemePreference> label="Theme" value={preference} onChange={setPreference} options={[{ value: "system", label: "Auto" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} />
    </Row>
  );
}

/* ------------------------------ answer rules ------------------------------ */

function RulesSection() {
  const [rules, setRules] = useState<AnswerRules | null>(null);
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);

  const load = useCallback(async () => {
    const result = await api.model.rules();
    if (result.ok) {
      setRules(result.value);
      setDraft(result.value.text);
    } else setError(result.error);
  }, []);
  useEffect(() => void load(), [load]);

  // Edits save themselves a moment after typing stops. Empty rules are never saved.
  const { state: saveState, flush } = useAutosave(
    draft,
    async (text) => {
      if (!text.trim()) return false;
      const result = await api.model.saveRules(text);
      if (!result.ok) {
        setError(result.error);
        return false;
      }
      setError(null);
      setRules(result.value);
      return true;
    },
    { enabled: open && rules !== null },
  );

  async function close() {
    await flush();
    setOpen(false);
  }

  async function reset() {
    setBusy(true);
    const result = await api.model.resetRules();
    setBusy(false);
    setConfirmReset(false);
    if (result.ok) {
      // Close first, so the original text isn't saved back as an edit.
      setOpen(false);
      setRules(result.value);
      setDraft(result.value.text);
      toast("Answer rules reset", { tone: "ok" });
    } else setError(result.error);
  }

  const status = saveState === "saving" || saveState === "pending" ? "Saving…" : saveState === "saved" ? "Saved" : saveState === "error" ? "Not saved" : null;
  return (
    <Section id="rules" title="Answer rules">
      <div className="set-row set-row--stack">
        <div className="set-row__label">
          <b>How every answer is written</b>
          <small>Applies to all copilots. Each copilot's guidance adds to these.</small>
        </div>
        <div className="set-rules">
          {rules ? <RichText text={rules.text} lines={2} /> : <Skeleton lines={2} />}
          <Button size="sm" variant="secondary" icon="edit" disabled={!rules} onClick={() => setOpen(true)}>Edit</Button>
        </div>
      </div>
      {error && !open ? <p className="set-row set-bad" role="alert">{error}</p> : null}

      <Sheet
        open={open}
        onClose={() => void close()}
        title="Answer rules"
        width={680}
        meta={status ?? `${wordCount(draft)} words`}
        footer={
          <>
            <Button variant="ghost" icon="undo" disabled={busy || !rules || rules.isDefault} onClick={() => setConfirmReset(true)}>Reset to original</Button>
            <span className="grow" />
            <Button variant="primary" onClick={() => void close()}>Done</Button>
          </>
        }
      >
        <TextEditor value={draft} onChange={(event) => setDraft(event.target.value)} aria-label="Answer rules" spellCheck={false} autoFocus />
        {!draft.trim() ? <p className="set-bad">Rules can't be empty. The last saved version is kept.</p> : null}
        {rules && !rules.meetsCacheFloor ? <p className="set-bad">These rules are short, so each answer costs a little more.</p> : null}
        {error ? <p className="set-bad">{error}</p> : null}
      </Sheet>

      <Dialog open={confirmReset} onClose={() => setConfirmReset(false)} size="sm" title="Reset answer rules?"
        footer={<><Button variant="ghost" onClick={() => setConfirmReset(false)}>Cancel</Button><Button variant="danger" busy={busy} onClick={() => void reset()}>Reset</Button></>}>
        <p>Your edits are replaced with the original rules.</p>
      </Dialog>
    </Section>
  );
}

/* -------------------------------- shortcuts -------------------------------- */

const SHORTCUTS: Array<[string, string, string]> = [
  ["⌘ ↩", "Answer", "During a call"],
  ["⌘ K", "Screenshot answer", "Anywhere, during a call"],
  ["⌘ ⇧ A", "Follow-up", "During a call"],
  ["⌘ [ ⌘ ]", "Previous or next answer", "During a call"],
  ["⌘ ⇧ H", "Hide or show Aside", "Anywhere"],
  ["⌘ P", "Search everything", "In this window"],
  ["⌘ \\", "Show or hide the sidebar", "In this window"],
];

function ShortcutsSection() {
  return (
    <Section id="shortcuts" title="Shortcuts">
      {SHORTCUTS.map(([keys, what, where]) => (
        <Row key={keys} label={what} hint={where}><Keys combo={keys} /></Row>
      ))}
    </Section>
  );
}
