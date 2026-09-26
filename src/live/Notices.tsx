import type { ReactNode } from "react";
import { Icon, type IconName } from "../kit/Icon";
import { useController, useLive } from "./context";
import { describeAnswerError, looksLikePermissionProblem } from "./errors";
import { answersOf } from "./machine";

interface AlertProps {
  tone: "warn" | "bad" | "note";
  icon: IconName;
  title: string;
  /** The one thing to do, always visible on the line. */
  fix?: { label: string; run: () => void };
  /** The why, and any other way out; shown when the line is hovered or focused. */
  detail?: ReactNode;
  more?: ReactNode;
  onClose?: () => void;
}

/** One line (icon, what's wrong, Fix). Hovering or tabbing into it opens the details underneath. */
function Alert({ tone, icon, title, fix, detail, more, onClose }: AlertProps) {
  const expandable = Boolean(detail || more);
  return (
    <div className="lv-alert" data-tone={tone} role={tone === "bad" ? "alert" : "status"} tabIndex={expandable ? 0 : undefined} data-expandable={expandable || undefined}>
      <div className="lv-alert__line">
        <Icon name={icon} size={15} />
        <b>{title}</b>
        {fix ? <button type="button" className="lv-chip" data-solid onClick={fix.run}>{fix.label}</button> : null}
        {onClose ? <button type="button" className="lv-icon lv-icon--sm" aria-label="Dismiss" onClick={onClose}><Icon name="x" size={14} /></button> : null}
      </div>
      {expandable ? (
        <div className="lv-alert__more">
          <div>
            {detail ? <p>{detail}</p> : null}
            {more ? <div className="lv-alert__actions">{more}</div> : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}

/**
 * Something needs the person's attention, said in plain words with the fix on the line.
 * Only the most urgent one shows at a time, and it goes away by itself once resolved.
 */
export function Notices() {
  const controller = useController();
  const system = useLive((state) => state.channels.system);
  const mic = useLive((state) => state.channels.mic);
  const offline = useLive((state) => state.offline);
  const notice = useLive((state) => state.notice);
  const lastFailed = useLive((state) => {
    const last = answersOf(state).at(-1);
    return last && last.status === "error" ? last : null;
  });

  if (mic.status === "error") {
    return (
      <Alert
        tone="bad"
        icon="alert"
        title="Your microphone isn't working"
        fix={{ label: "Try again", run: () => void controller.toggleChannel("mic") }}
        detail={mic.message ?? "Can't open the microphone."}
        more={<button type="button" className="lv-chip" onClick={() => void controller.backend.openPrivacySettings("microphone")}>Open System Settings</button>}
      />
    );
  }

  if (system.status === "silent" || system.status === "error") {
    const permission = looksLikePermissionProblem(system.message);
    const checkAgain = async () => {
      if (system.status === "silent") await controller.toggleChannel("system");
      await controller.toggleChannel("system");
    };
    const openSettings = () => void controller.backend.openPrivacySettings("screen");
    return (
      <Alert
        tone="warn"
        icon="alert"
        title="Can't hear the other side"
        fix={permission ? { label: "Open Settings", run: openSettings } : { label: "Check again", run: () => void checkAgain() }}
        detail={<>{permission ? "Allow Aside under Screen & System Audio Recording, then check again." : (system.message ?? "The system audio stream stopped.")} On AirPods? Use the built-in mic.</>}
        more={permission
          ? <button type="button" className="lv-chip" onClick={() => void checkAgain()}>Check again</button>
          : <button type="button" className="lv-chip" onClick={openSettings}>Open System Settings</button>}
      />
    );
  }

  if (offline) {
    const friendly = describeAnswerError(offline);
    return (
      <Alert
        tone="bad"
        icon="wifioff"
        title={friendly.title}
        fix={lastFailed ? { label: "Retry", run: () => void controller.retry(lastFailed) } : undefined}
        detail="Listening continues. Answers are paused until Aside reconnects."
      />
    );
  }

  if (notice) {
    return <Alert tone="note" icon="info" title={notice} onClose={() => controller.dispatch({ type: "dismiss-notice" })} />;
  }
  return null;
}
