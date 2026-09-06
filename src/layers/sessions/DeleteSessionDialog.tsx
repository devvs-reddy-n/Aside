import { useState } from "react";
import { Dialog, Button } from "../../ui";
import type { PastSessionSummary } from "../../platform/types";

export interface DeleteSessionDialogProps {
  session: PastSessionSummary | null;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}

/** Permanent, cascading delete (`delete_session` removes the transcript
 *  rows too) — always confirm first. */
export function DeleteSessionDialog({ session, onClose, onConfirm }: DeleteSessionDialogProps) {
  const [deleting, setDeleting] = useState(false);

  return (
    <Dialog
      open={session != null}
      onClose={onClose}
      title="Delete this session?"
      destructive
      footer={
        <>
          <Button variant="secondary" onClick={onClose} disabled={deleting}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            disabled={deleting}
            onClick={async () => {
              setDeleting(true);
              await onConfirm();
              setDeleting(false);
            }}
          >
            {deleting ? "Deleting…" : "Delete"}
          </Button>
        </>
      }
    >
      <p>
        This permanently removes “{session?.title}” and its saved transcript. This can't be undone.
      </p>
    </Dialog>
  );
}
