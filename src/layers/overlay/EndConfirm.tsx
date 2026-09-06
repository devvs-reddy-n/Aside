export interface EndConfirmProps {
  onConfirm: () => void;
  onCancel: () => void;
}

/** Spec §12 state 15 — the Timer/End pill's confirm step. */
export function EndConfirm({ onConfirm, onCancel }: EndConfirmProps) {
  return (
    <div className="ov-end-confirm" role="dialog" aria-modal="true">
      <div className="ov-end-confirm__card">
        <p className="ov-end-confirm__title">End this session?</p>
        <p className="ov-end-confirm__desc">
          This stops listening on both the microphone and system audio, and closes this window.
        </p>
        <div className="ov-end-confirm__actions">
          <button type="button" className="ov-end-confirm__cancel" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="ov-end-confirm__confirm" onClick={onConfirm}>
            End Session
          </button>
        </div>
      </div>
    </div>
  );
}
