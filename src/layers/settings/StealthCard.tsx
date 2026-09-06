import { useState } from "react";
import { Badge, Button, Card, Dialog } from "../../ui";
import { useStealth } from "../../state/stealth";
import { isTauriRuntime } from "../../platform/runtime";
import { SettingsBullets } from "./SettingsBullets";

/**
 * Hide Aside from screenshots, recordings, meeting screen-shares, the
 * Dock, and the menu bar. This Settings card is the only on/off.
 * Changing it relaunches Aside, so flipping it first asks — the same
 * "are you sure, this closes and reopens Aside" shape as Quit
 * (`QuitConfirm.tsx`) — rather than closing and reopening the moment the
 * checkbox is touched. ⌘⇧H is a different control: it hides Aside from
 * *your* screen without changing this.
 */
export function StealthCard() {
  const stealth = useStealth();
  const tauri = isTauriRuntime();
  const [pending, setPending] = useState<boolean | null>(null);

  return (
    <Card
      className="layer-page__section"
      title="Hide from screen sharing"
      footer={
        <Badge tone={!tauri ? "bad" : stealth.enabled ? "ok" : "warn"}>
          {!tauri ? "Not available" : stealth.enabled ? "Hidden" : "Visible"}
        </Badge>
      }
    >
      <div className="step-body">
        {!tauri ? (
          <SettingsBullets items={["Open Aside.app to change this."]} />
        ) : (
          <>
            <label className="stealth-toggle">
              <span>
                <strong>Hide from screen sharing, Dock, and menu bar</strong>
              </span>
              <input
                type="checkbox"
                checked={stealth.enabled}
                onChange={(event) => setPending(event.target.checked)}
              />
            </label>
            <SettingsBullets
              items={[
                "Stays on your screen. Hidden from meetings, screenshots, Dock, and the menu bar.",
                "While hidden, Aside also stays on top of every other window — ⌘⇧H is the only thing that hides it from you.",
                "Changing this closes Aside and opens it again.",
              ]}
            />
          </>
        )}
      </div>
      <Dialog
        open={pending !== null}
        onClose={() => setPending(null)}
        title={pending ? "Turn on hiding?" : "Turn off hiding?"}
        footer={
          <>
            <Button variant="secondary" onClick={() => setPending(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                const next = pending;
                setPending(null);
                if (next !== null) void stealth.setEnabled(next);
              }}
            >
              {pending ? "Turn on and reopen" : "Turn off and reopen"}
            </Button>
          </>
        }
      >
        <p>
          {pending
            ? "Aside closes and opens again so it's actually left out of screenshots, recordings, and meeting shares from the first frame."
            : "Aside closes and opens again. Once it's back, meetings and screenshots will be able to see it."}
        </p>
      </Dialog>
    </Card>
  );
}
