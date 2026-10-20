import { useCallback, useEffect, useState, type ReactNode } from "react";
import * as api from "../../bridge/api";
import { backendAvailable } from "../../bridge/runtime";
import type { AnswerRules, EngineKind, LlmProviderKind, OverlayPrefs } from "../../bridge/types";
import { useStore } from "../../core/store";
import { Badge, Callout } from "../../kit/Bits";
import { Button } from "../../kit/Button";
import { Dialog } from "../../kit/Dialog";
import { Segmented, Select, TextInput, Toggle } from "../../kit/Form";
import { Icon, type IconName } from "../../kit/Icon";
import { Sheet } from "../../kit/Sheet";
import { TextEditor } from "../../kit/TextEditor";
import { useAutosave } from "../../kit/useAutosave";
import { useStealth } from "../../kit/Stealth";
import { wordCount } from "../../core/text";
import { toast } from "../../kit/toast";
import { refreshSystem, systemStore } from "../data";
import { ENGINE_ORDER, LANGUAGES, engineName, engineNeedsKey, modelLabel, providerLabel } from "../labels";

const SECTIONS = ["speech", "ai", "rules", "general"] as const;

/** Familiar grouped preferences with enough room for complete control values. */
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
      <div className="set-grid">
        <Section id="general" title="Privacy" icon="shield" footer="Stealth keeps Aside out of screenshots and shared screens."><PrivacyRow /></Section>
        <AiSection />
        <SpeechSection />
      </div>
      <RulesSection />
    </div>
  );
}

/** One section card: a small icon beside the title, then its rows. The badge only appears when something needs doing. */
function Section({ id, title, icon, badge, action, footer, children }: { id: string; title: string; icon: IconName; badge?: ReactNode; action?: ReactNode; footer?: ReactNode; children: ReactNode }) {
  return (
    <section className="set-sec" id={`st-${id}`} aria-labelledby={`st-${id}-title`}>
      <header className="set-sec__head">
        <div className="set-sec__label"><Icon name={icon} size={15} /><h2 className="set-sec__title" id={`st-${id}-title`}>{title}{badge}</h2></div>
        {action}
      </header>
      <div className="set-card">{children}</div>
      {footer ? <p className="group-help">{footer}</p> : null}
    </section>
  );
}

/** Controls stay in the group; explanations belong in its footer. */
function Row({ label, children }: { label: string; children?: ReactNode }) {
  return (
    <div className="set-row">
      <div className="set-row__label"><b>{label}</b></div>
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
      <div className="set-row__label"><b>{label}</b>{error ? <small className="set-bad">Couldn't save that key.</small> : null}</div>
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
  const engines = system.engines.length > 0 ? system.engines : ENGINE_ORDER;

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
    <Section id="speech" title="Speech" icon="mic" footer={local && !system.parakeetReady ? "One-time download, about 480 MB." : null} badge={system.engine && !ready ? <Badge tone="warn">{local ? "Needs download" : "Needs key"}</Badge> : null}>
      <Row label="Engine">
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
        <Row label="Speech model">
          <Button size="sm" variant="primary" busy={downloading} onClick={() => void download()}>{downloading ? "Downloading" : "Download"}</Button>
        </Row>
      ) : null}

      {!local ? (
        <Row label="Language">
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

const PROVIDERS: LlmProviderKind[] = ["open_ai", "anthropic"];

/**
 * Works like the speech engine switch: pick OpenAI or Claude, then one of its models.
 * Copilots only offer the chosen provider's models. Background call notes always run on
 * OpenAI, so with Claude chosen the OpenAI key is still asked for.
 */
function AiSection() {
  const system = useStore(systemStore, (state) => state);
  const [options, setOptions] = useState<ModelOption[]>([]);
  const [keys, setKeys] = useState<Partial<Record<LlmProviderKind, boolean>>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function refreshKeys() {
    const saved = await Promise.all(PROVIDERS.map(async (provider) => {
      const result = await api.model.hasKey(provider);
      return [provider, result.ok && result.value] as const;
    }));
    setKeys(Object.fromEntries(saved));
  }

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
      if (alive) await refreshKeys();
    })();
    return () => {
      alive = false;
    };
  }, []);

  async function run(step: () => Promise<{ ok: boolean; error?: string }>) {
    setBusy(true);
    setError(null);
    const result = await step();
    setBusy(false);
    if (!result.ok) setError(result.error ?? "Could not save that choice.");
    await refreshSystem();
  }

  const provider = system.provider ?? "open_ai";
  const models = options.filter((option) => option.provider === provider);
  const saveKey = (each: LlmProviderKind) => async (key: string) => {
    const result = await api.model.saveKey(each, key);
    await refreshKeys();
    await refreshSystem();
    return result.ok;
  };
  return (
    <Section id="ai" title="AI model" icon="bot" badge={system.model && !system.aiKey ? <Badge tone="warn">Needs key</Badge> : null}>
      <Row label="Provider">
        <Segmented<LlmProviderKind> label="AI provider" value={provider} onChange={(next) => void run(() => api.model.setProvider(next))} options={PROVIDERS.map((each) => ({ value: each, label: providerLabel(each) }))} />
      </Row>
      <Row label="Default model">
        <Select aria-label="AI model" disabled={busy || models.length === 0} value={system.model ? `${provider}|${system.model}` : ""} onChange={(event) => { const model = event.target.value.split("|").slice(1).join("|"); void run(() => api.model.setModel(provider, model)); }}>
          <option value="" disabled>Choose one</option>
          {models.map((option) => <option key={option.model} value={`${option.provider}|${option.model}`}>{modelLabel(option.provider, option.model)}</option>)}
        </Select>
      </Row>
      <KeyRow key={provider} label={`${providerLabel(provider)} key`} hasKey={keys[provider] ?? false} onSave={saveKey(provider)} />
      {provider === "anthropic" ? <WorkspaceRow /> : null}
      {provider === "anthropic" ? (
        <KeyRow key="notes" label="OpenAI key (call notes)" hasKey={keys.open_ai ?? false} onSave={saveKey("open_ai")} />
      ) : null}
      {error ? <p className="set-row set-bad" role="alert">{error}</p> : null}
    </Section>
  );
}

/**
 * Some Claude keys aren't tied to one workspace; those need the workspace's ID on every request.
 * A key created for a single workspace leaves this empty.
 */
function WorkspaceRow() {
  const [saved, setSaved] = useState("");
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void api.model.workspace().then((result) => {
      if (result.ok) {
        setSaved(result.value);
        setValue(result.value);
      }
    });
  }, []);

  async function save() {
    setBusy(true);
    setError(null);
    const result = await api.model.setWorkspace(value.trim());
    setBusy(false);
    if (!result.ok) return setError(result.error);
    setSaved(value.trim());
    toast(value.trim() ? "Workspace saved" : "Workspace cleared", { tone: "ok" });
    await refreshSystem();
  }

  return (
    <div className="set-row set-row--stack">
      <div className="set-row__label"><b>Claude workspace ID (only if your key needs one)</b>{error ? <small className="set-bad" role="alert">{error}</small> : null}</div>
      <div className="set-inline">
        <TextInput autoComplete="off" spellCheck={false} value={value} placeholder="wrkspc_…" aria-label="Claude workspace ID" onChange={(event) => setValue(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void save(); }} />
        <Button variant="primary" busy={busy} disabled={value.trim() === saved} onClick={() => void save()}>Save</Button>
      </div>
    </div>
  );
}

/* --------------------------------- privacy --------------------------------- */

function PrivacyRow() {
  const stealth = useStealth();
  const hidden = useStore(systemStore, (state) => state.hidden);
  const [pending, setPending] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);

  async function confirm() {
    if (pending === null) return;
    setBusy(true);
    const next = pending;
    setPending(null);
    // The backend saves the choice and reopens Aside so it applies from the first frame.
    const result = await api.stealth.set(next);
    if (!result.ok) toast(result.error, { tone: "bad" });
    else {
      await stealth.refresh();
      await refreshSystem();
    }
    setBusy(false);
  }

  return (
    <>
      <Row label="Stealth">
        <Toggle checked={hidden ?? true} label="Stealth" disabled={hidden === null || busy} onChange={(next) => setPending(next)} />
      </Row>
      <Dialog
        open={pending !== null}
        onClose={() => setPending(null)}
        size="sm"
        title={pending ? "Turn on Stealth?" : "Turn off Stealth?"}
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

/* ------------------------------ system rules ------------------------------ */

function RulesSection() {
  const [rules, setRules] = useState<AnswerRules | null>(null);
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmEdit, setConfirmEdit] = useState(false);
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
      toast("System rules reset to default", { tone: "ok" });
    } else setError(result.error);
  }

  const status = saveState === "saving" || saveState === "pending" ? "Saving…" : saveState === "saved" ? "Saved" : saveState === "error" ? "Not saved" : null;
  return (
    // Just the button: the rules themselves are only shown after the warning.
    <div className="set-rules set-rules--button" id="st-rules">
      <button type="button" className="set-rules__edit" disabled={!rules} onClick={() => setConfirmEdit(true)}>Edit system rules</button>
      {error && !open ? <p className="set-row set-bad" role="alert">{error}</p> : null}

      <Dialog open={confirmEdit} onClose={() => setConfirmEdit(false)} title="Edit the system rules?" size="sm"
        footer={<><Button variant="ghost" onClick={() => setConfirmEdit(false)}>Cancel</Button><Button variant="danger" onClick={() => { setConfirmEdit(false); setOpen(true); }}>Edit rules</Button></>}>
        <p>System rules decide what kind of answer the AI gives (answering a question, replying to an introduction, finishing your point, length), in every copilot. A small change can make answers worse. You can reset them to the default at any time.</p>
      </Dialog>

      <Sheet
        open={open}
        onClose={() => void close()}
        title="System rules"
        width={680}
        meta={status ?? `${wordCount(draft)} words`}
        footer={
          <>
            <Button variant="ghost" icon="undo" disabled={busy || !rules || rules.isDefault} onClick={() => setConfirmReset(true)}>Reset to default</Button>
            <span className="grow" />
            <Button variant="primary" onClick={() => void close()}>Done</Button>
          </>
        }
      >
        <TextEditor value={draft} onChange={(event) => setDraft(event.target.value)} aria-label="System rules" spellCheck={false} autoFocus />
        {!draft.trim() ? <p className="set-bad">Rules can't be empty. The last saved version is kept.</p> : null}
        {rules && !rules.meetsCacheFloor ? <p className="set-bad">These rules are short, so each answer costs a little more.</p> : null}
        {error ? <p className="set-bad">{error}</p> : null}
      </Sheet>

      <Dialog open={confirmReset} onClose={() => setConfirmReset(false)} size="sm" title="Reset system rules to default?"
        footer={<><Button variant="ghost" onClick={() => setConfirmReset(false)}>Cancel</Button><Button variant="danger" busy={busy} onClick={() => void reset()}>Reset</Button></>}>
        <p>Your edits are replaced with the default system rules.</p>
      </Dialog>
    </div>
  );
}
