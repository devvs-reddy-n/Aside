import { useEffect, useState } from "react";
import { Icon } from "../../kit/Icon";
import { Button } from "../../kit/Button";
import { Dialog } from "../../kit/Dialog";
import { onEvent } from "../../bridge/events";
import { useController, useLiveUi } from "../context";

/** Ending is an explicit choice; cancelling leaves listening running. */
export function End() {
  const controller = useController();
  const ending = useLiveUi((state) => state.ending);
  const [asking, setAsking] = useState(false);
  useEffect(() => onEvent("end-requested", () => setAsking(true)), []);
  return <>
    <button type="button" className="lv-end" disabled={ending} onClick={() => setAsking(true)} aria-label="End session" title="End session">
      <Icon name="stop" size={13} /><span>{ending ? "Ending…" : "End"}</span>
    </button>
    <Dialog open={asking} onClose={() => setAsking(false)} title="End this call?" size="sm"
      footer={<><Button variant="ghost" onClick={() => setAsking(false)}>Keep going</Button><Button className="btn--end-quit" variant="danger" busy={ending} onClick={() => { setAsking(false); void controller.end(); }}>End call</Button></>}>
      <p>Listening stops. Saved conversations and answers stay in History.</p>
    </Dialog>
  </>;
}
