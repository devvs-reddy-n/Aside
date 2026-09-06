import { useCallback, useState } from "react";
import { useAudioChannel } from "../../state/audioChannel";
import { invoke } from "../../platform/invoke";
import { Icon } from "../../ui/Icon";
import type { Session } from "../../platform/types";

export interface ConnectStepProps {
  onBack: () => void;
  /** The session `useCreateForm.submit()` already created (Create's own
   *  CTA does the actual `create_session` call before switching to this
   *  step) — Connect's job is only to start capture and open the overlay
   *  for it, spec §5's "Connect starts mic + system capture, opens the
   *  overlay, hides Create." */
  session: Session | null;
}

/**
 * Screen B — Connect (spec §5). The callout is deliberately the *opposite*
 * of Natively/OpenCluely's real Connect copy (which tells the user how to
 * hide from screen share) — Aside states plainly that the helper stays
 * visible. This is a hard ethical requirement (`CLAUDE.md`), not a style
 * choice.
 *
 * **Phase B**: real, not stubbed. Mirrors exactly what
 * `LiveAnswerLayer.tsx`'s `handleStart` already does to begin a live
 * session (start both capture channels, then `start_channel_transcription`
 * on each), then additionally opens the overlay window
 * (`commands::overlay::open_overlay`) and hides this window
 * (`commands::overlay::hide_main_window` — hides, does not destroy, so
 * Create's own form state is exactly where it was if the person ever comes
 * back via Edit/Dashboard).
 */
export function ConnectStep({ onBack, session }: ConnectStepProps) {
  const mic = useAudioChannel("mic");
  const system = useAudioChannel("system");
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleConnect = useCallback(async () => {
    if (!session) {
      setError("No session to connect. Go back and create one first.");
      return;
    }
    setConnecting(true);
    setError(null);

    const micOk = mic.capturing || (await mic.start());
    if (!micOk) {
      setError(mic.error ?? "Could not start the microphone.");
      setConnecting(false);
      return;
    }
    const systemOk = system.capturing || (await system.start());
    if (!systemOk) {
      setError(system.error ?? "Could not start system audio.");
      setConnecting(false);
      return;
    }

    // `sessionId: null` is what actually makes "Save Transcript" off do
    // anything real — `pipeline::start`'s whole persistence path (writing
    // final segments to `storage/transcripts.rs`, indexing them for RAG) is
    // gated on `session_id: Option<Uuid>` being `Some`. Passing the real id
    // unconditionally here, regardless of the checkbox, was a real gap: the
    // database row already carries `save_transcript`, but nothing ever
    // consulted it before this fix, so unchecking the box saved anyway.
    const persistedSessionId = session.save_transcript ? session.id : null;
    const micStt = await invoke<void>("start_channel_transcription", {
      channel: "mic",
      sessionId: persistedSessionId,
    });
    const systemStt = await invoke<void>("start_channel_transcription", {
      channel: "system",
      sessionId: persistedSessionId,
    });
    if (!micStt.ok) {
      setError(micStt.error);
      setConnecting(false);
      return;
    }
    if (!systemStt.ok) {
      setError(systemStt.error);
      setConnecting(false);
      return;
    }

    const opened = await invoke<void>("open_overlay");
    if (!opened.ok) {
      setError(opened.error);
      setConnecting(false);
      return;
    }
    await invoke<void>("hide_main_window");
    setConnecting(false);
  }, [session, mic, system]);

  return (
    <div className="sc-connect">
      <p className="sc-connect__desc">This is an unlimited session. It will end when inactivity is detected.</p>

      <div className="sc-callout">
        <span className="sc-callout__icon" aria-hidden="true">
          <Icon name="info" size={14} />
        </span>
        <div>
          <p style={{ margin: 0 }}>
            Make sure this Mac can use the microphone and system audio. Test on a quiet call with a friend
            before a real one.
          </p>
          <button type="button" className="sc-video-link">
            Video Tutorial
          </button>
          <p className="sc-callout__honest">
            This helper stays visible. If you share your screen, the other person can see it.
          </p>
        </div>
      </div>

      {error ? <p className="sc-connect-note">{error}</p> : null}

      <div className="sc-connect__actions">
        <button type="button" className="sc-back-btn" onClick={onBack} disabled={connecting}>
          Back
        </button>
        <button type="button" className="sc-cta" onClick={() => void handleConnect()} disabled={connecting || !session}>
          <span>{connecting ? "Connecting…" : "Connect"}</span>
          {connecting ? null : <Icon name="arrowRight" size={14} />}
        </button>
      </div>
    </div>
  );
}
