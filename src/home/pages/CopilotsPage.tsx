import { useState } from "react";
import * as api from "../../bridge/api";
import type { Copilot, CopilotSummary } from "../../bridge/types";
import { useStore } from "../../core/store";
import { EmptyState } from "../../kit/Bits";
import { Button } from "../../kit/Button";
import { toast } from "../../kit/toast";
import { deleteWithUndo } from "../../kit/undo";
import { copilotsStore, loadCopilots, setHidden } from "../data";
import { navigate } from "../router";
import { CopilotForm } from "./CopilotForm";
import { StartDialog } from "./StartDialog";

/** The main pane for Copilots: the chosen copilot (picked in the sidebar), a new one, or a first-run welcome. */
export function CopilotsPage({ selectedId }: { selectedId: string | null }) {
  const { items, loaded, hiddenIds } = useStore(copilotsStore, (state) => state);
  const [starting, setStarting] = useState<CopilotSummary | Copilot | null>(null);
  const visible = items.filter((copilot) => !hiddenIds.includes(copilot.id));
  const creating = selectedId === "new";
  const selected = creating ? null : visible.find((copilot) => copilot.id === selectedId) ?? visible[0] ?? null;

  function remove(copilot: CopilotSummary) {
    navigate("copilots");
    deleteWithUndo({
      label: `“${copilot.company || copilot.name}”`,
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

  if (!loaded) return null;
  return (
    <section className="hm-detail">
      {creating ? (
        <CopilotForm key="new" copilot={null} onCancelNew={() => navigate("copilots")} onSaved={(saved) => navigate("copilots", saved.id)} />
      ) : selected ? (
        <CopilotForm key={selected.id} copilot={selected} onDelete={() => remove(selected)} onSaved={() => undefined} onStart={setStarting} />
      ) : (
        <EmptyState icon="spark" title="Create your first copilot" action={<div className="empty__actions"><Button variant="primary" icon="plus" onClick={() => navigate("copilots", "new")}>New copilot</Button></div>}>
          A copilot holds one job: the role, your resume and how you like to answer.
        </EmptyState>
      )}
      {starting ? <StartDialog copilot={starting as CopilotSummary} onClose={() => setStarting(null)} /> : null}
    </section>
  );
}
