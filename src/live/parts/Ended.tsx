import { formatClock } from "../../core/format";
import { Icon } from "../../kit/Icon";
import { useLive } from "../context";

/** Shown over everything once the call is over. */
export function Ended() {
  const ended = useLive((state) => state.ended);
  if (!ended) return null;
  return (
    <div className="lv-ended" role="status">
      <span className="lv-ended__check"><Icon name="check" size={26} strokeWidth={2.25} /></span>
      <h2>Session ended</h2>
      <p>Open History in Aside to review your saved sessions.</p>
      <dl>
        <div><dt>Length</dt><dd className="tabular">{formatClock(ended.elapsedMs)}</dd></div>
        <div><dt>Turns</dt><dd className="tabular">{ended.turns}</dd></div>
        <div><dt>Answers</dt><dd className="tabular">{ended.answers}</dd></div>
      </dl>
    </div>
  );
}
