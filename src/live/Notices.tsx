import type { ReactNode } from "react";
import { Icon, type IconName } from "../kit/Icon";
import { useController, useLive } from "./context";
import { describeAnswerError, looksLikePermissionProblem } from "./errors";
import { answersOf } from "./machine";

function Notice({ tone, icon, title, children, actions, onClose }: { tone: "warn" | "bad" | "note"; icon: IconName; title?: string; children: ReactNode; actions?: ReactNode; onClose?: () => void }) {
  return (
    <div className="lv-notice" data-tone={tone} role={tone === "bad" ? "alert" : "status"}>
      <Icon name={icon} size={17} />
      <div className="lv-notice__text">
        {title ? <b>{title}</b> : null}
        <p>{children}</p>
        {actions ? <div className="lv-notice__actions">{actions}</div> : null}
      </div>
      {onClose ? (
        <button type="button" className="lv-icon lv-icon--sm" aria-label="Dismiss" onClick={onClose}><Icon name="x" size={14} /></button>
      ) : null}
    </div>
  );
}

/**
 * Something needs the person's attention: said in plain words, with the fix
 * on the card. Only the most urgent one shows at a time.
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
      <Notice
        tone="bad"
        icon="alert"
        title="Your microphone isn't working"
        actions={
          <>
            <button type="button" className="lv-chip" onClick={() => void controller.backend.openPrivacySettings("microphone")}>Open System Settings</button>
            <button type="button" className="lv-chip" data-solid onClick={() => void controller.toggleChannel("mic")}>Try again</button>
          </>
        }
      >
        {mic.message ?? "Can't open the microphone."}
      </Notice>
    );
  }

  if (system.status === "silent" || system.status === "error") {
    const permission = looksLikePermissionProblem(system.message);
    return (
      <Notice
        tone="warn"
        icon="alert"
        title="Can't hear the other side"
        actions={
          <>
            <button type="button" className="lv-chip" onClick={() => void controller.backend.openPrivacySettings("screen")}>Open System Settings</button>
            <button
              type="button"
              className="lv-chip"
              data-solid
              onClick={async () => {
                if (system.status === "silent") await controller.toggleChannel("system");
                await controller.toggleChannel("system");
              }}
            >
              Check again
            </button>
          </>
        }
      >
        {permission
          ? "Allow Aside under Screen & System Audio Recording, then reopen."
          : (system.message ?? "The system audio stream stopped.")}{" "}
        On AirPods? Use the built-in mic.
      </Notice>
    );
  }

  if (offline) {
    const friendly = describeAnswerError(offline);
    return (
      <Notice
        tone="bad"
        icon="wifioff"
        title={friendly.title}
        actions={lastFailed ? <button type="button" className="lv-chip" data-solid onClick={() => void controller.retry(lastFailed)}>Retry now</button> : undefined}
      >
        Listening continues. Answers are paused.
      </Notice>
    );
  }

  if (notice) {
    return (
      <Notice tone="note" icon="info" onClose={() => controller.dispatch({ type: "dismiss-notice" })}>
        {notice}
      </Notice>
    );
  }
  return null;
}
