import { useEffect, useMemo, useState } from "react";
import * as api from "../../bridge/api";
import type { Copilot, CopilotSummary } from "../../bridge/types";
import { formatRelative } from "../../core/format";
import { useStore } from "../../core/store";
import { Callout, EmptyState } from "../../kit/Bits";
import { Button } from "../../kit/Button";
import { Icon } from "../../kit/Icon";
import { deleteWithUndo } from "../../kit/undo";
import { toast } from "../../kit/toast";
import { copilotsStore, loadCopilots, setHidden } from "../data";
import { hrefFor, navigate } from "../router";
import { CopilotForm } from "./CopilotForm";
import { StartDialog } from "./StartDialog";

export function CopilotsPage({ selectedId }: { selectedId: string | null }) {
  const { items, loaded, error, hiddenIds } = useStore(copilotsStore, (state) => state);
  const [query, setQuery] = useState("");
  const [creating, setCreating] = useState(false);
  /** The copilot whose Start window is open. */
  const [starting, setStarting] = useState<CopilotSummary | Copilot | null>(null);

  useEffect(() => {
    void loadCopilots();
  }, []);

  const visible = useMemo(() => {
    const term = query.trim().toLowerCase();
    return items.filter((copilot) => {
      if (hiddenIds.includes(copilot.id)) return false;
      if (!term) return true;
      return [copilot.name, copilot.company, copilot.target_role].filter(Boolean).join(" ").toLowerCase().includes(term);
    });
  }, [items, hiddenIds, query]);

  const selected = visible.find((copilot) => copilot.id === selectedId) ?? visible[0] ?? null;
  const total = items.length - hiddenIds.length;

  function remove(copilot: CopilotSummary) {
    deleteWithUndo({
      label: `“${copilot.name}”`,
      hide: () => setHidden(copilotsStore, copilot.id, true),
      restore: () => setHidden(copilotsStore, copilot.id, false),
      commit: async () => {
        const result = await api.copilots.remove(copilot.id);
        if (result.ok) {
          await loadCopilots();
          setHidden(copilotsStore, copilot.id, false);
        }
        return result.ok;
      },
      onFailed: (message) => toast(message, { tone: "bad" }),
    });
  }

  return (
    <div className="hm-split">
      <section className="hm-list" aria-label="Copilots">
        <header className="hm-list__head">
          <h1>Copilots</h1>
          <Button variant="secondary" size="sm" icon="plus" onClick={() => setCreating(true)}>New</Button>
        </header>
        <label className="hm-search">
          <Icon name="search" size={16} />
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search" aria-label="Find a copilot" />
        </label>
        {error ? <Callout tone="bad" title="Couldn't load your copilots">{error}</Callout> : null}
        <ul className="hm-rows">
          {visible.map((copilot) => (
            <li key={copilot.id}>
              <a className="hm-row" href={hrefFor("copilots", copilot.id)} aria-current={selected?.id === copilot.id ? "true" : undefined}>
                <span className="hm-avatar">{(copilot.company || copilot.name).slice(0, 1).toUpperCase()}</span>
                <span className="hm-row__text">
                  <b>{copilot.company || copilot.name}</b>
                  <small>{copilot.target_role || "No role"}</small>
                </span>
                <small className="hm-row__when">{formatRelative(copilot.updated_at_ms)}</small>
              </a>
            </li>
          ))}
        </ul>
        {loaded && visible.length === 0 && total > 0 ? <p className="hm-muted hm-pad">No copilot matches “{query}”.</p> : null}
      </section>

      <section className="hm-detail">
        {!loaded ? null : creating ? (
          <CopilotForm
            key="new"
            copilot={null}
            onCancelNew={() => setCreating(false)}
            onSaved={(saved) => {
              setCreating(false);
              navigate("copilots", saved.id);
              toast("Created.", { tone: "ok" });
            }}
          />
        ) : selected ? (
          <CopilotForm
            key={selected.id}
            copilot={selected}
            onDelete={() => remove(selected)}
            onSaved={() => toast("Saved.", { tone: "ok" })}
            onStart={setStarting}
          />
        ) : (
          <EmptyState icon="spark" title="Create a copilot" action={<Button variant="primary" icon="plus" onClick={() => setCreating(true)}>New copilot</Button>}>
            A saved setup for one kind of call.
          </EmptyState>
        )}
      </section>

      {starting ? <StartDialog copilot={starting as CopilotSummary} onClose={() => setStarting(null)} /> : null}
    </div>
  );
}
