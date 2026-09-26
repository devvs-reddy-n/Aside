import { useRef, useState } from "react";
import * as api from "../../bridge/api";
import type { Copilot, CopilotInput, CopilotSummary } from "../../bridge/types";
import { lengthFor, setLengthFor } from "../../core/answerLength";
import { useStore } from "../../core/store";
import { EmptyState } from "../../kit/Bits";
import { Button } from "../../kit/Button";
import { toast } from "../../kit/toast";
import { deleteWithUndo } from "../../kit/undo";
import { copilotsStore, loadCopilots, saveCopilot, setHidden } from "../data";
import { navigate } from "../router";
import { CopilotPage } from "./CopilotPage";
import { StartSheet } from "./StartSheet";

/**
 * A new copilot keeps its on-screen identity after its first save, so the page doesn't
 * blink or lose focus while the address changes from "new" to the copilot's id.
 */
const keyFor = new Map<string, string>();
let drafts = 0;

/** The main pane for Copilots: the chosen copilot (picked in the sidebar), a new one, or a first-run welcome. */
export function CopilotsPage({ selectedId }: { selectedId: string | null }) {
  const { items, loaded, hiddenIds } = useStore(copilotsStore, (state) => state);
  const [starting, setStarting] = useState<CopilotSummary | Copilot | null>(null);
  const [linkFirst, setLinkFirst] = useState(false);
  const visible = items.filter((copilot) => !hiddenIds.includes(copilot.id));
  const creating = selectedId === "new";
  const selected = creating ? null : visible.find((copilot) => copilot.id === selectedId) ?? visible[0] ?? null;

  // A fresh draft key each time "new" is opened.
  const draftKey = useRef("");
  const wasCreating = useRef(false);
  if (creating && !wasCreating.current) draftKey.current = `draft-${++drafts}`;
  wasCreating.current = creating;

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

  async function duplicate(from: CopilotSummary, copy: CopilotInput) {
    const result = await saveCopilot(null, copy);
    if (!result.ok) {
      toast(result.error, { tone: "bad" });
      return;
    }
    setLengthFor(result.value.id, lengthFor(from.id));
    toast("Copilot duplicated", { tone: "ok" });
    navigate("copilots", result.value.id);
  }

  function newCopilot(withLink: boolean) {
    setLinkFirst(withLink);
    navigate("copilots", "new");
  }

  if (!loaded) return null;
  return (
    <section className="hm-detail">
      {creating || selected ? (
        <CopilotPage
          key={creating ? draftKey.current : keyFor.get(selected!.id) ?? selected!.id}
          copilot={creating ? null : selected}
          startWithLink={creating && linkFirst}
          onCreated={(saved) => {
            keyFor.set(saved.id, draftKey.current);
            navigate("copilots", saved.id);
          }}
          onDelete={() => selected && remove(selected)}
          onDuplicate={(copy) => selected && void duplicate(selected, copy)}
          onStart={setStarting}
        />
      ) : (
        <EmptyState
          icon="spark"
          title="Create your first copilot"
          action={
            <div className="empty__actions">
              <Button variant="primary" icon="plus" onClick={() => newCopilot(false)}>New copilot</Button>
              <Button variant="secondary" icon="link" onClick={() => newCopilot(true)}>Start from job link</Button>
            </div>
          }
        >
          A copilot holds one job: the role, your resume and how you like to answer.
        </EmptyState>
      )}
      {starting ? <StartSheet copilot={starting as CopilotSummary} onClose={() => setStarting(null)} /> : null}
    </section>
  );
}
