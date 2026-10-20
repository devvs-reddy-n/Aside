# Current status

**BUILT + VERIFIED — grey live-session reimplementation and light workspace (2026-10-17). PARTIAL — reliable native audio startup. PARTIAL — Speechmatics live connection rework (2026-10-17).**

## Current result

- Answers (2026-10-20): one System rules set (editable, Reset to default; your earlier rules kept under MY CONTENT RULES); each request labels rules, guidance, target role and experience; calls always use the copilot's current documents; all dossiers sent; prompt caching verified live (~10% cost per answer after a one-time warm-up). Answers still sometimes run longer than the length bands.

- Models (2026-10-17): Settings switch between OpenAI (GPT-6 Luna, GPT-6.1 Sol, GPT-5.6 Terra) and Claude (Opus 5.5, Sonnet 5.5, Haiku 5.5); Copilots only offer the chosen provider. Screenshots use the call's model; speech is Speechmatics or Parakeet only. Background notes still need an OpenAI key. No real GPT-6 or Claude answer tested yet.

- Answer prompt (2026-10-17): your Strict rules and copilot Guidance now lead every answer, including screenshots; your actual words beat any AI suggestion; full resume, job description and target role are sent. Tests and builds pass; a real answer with the new prompt has not been checked yet.

- Speechmatics (2026-10-17): mic/speaker only show as listening once Speechmatics confirms the session; a new session waits for the old one to close (no overlapping sessions); dropped connections resend only unheard audio; the last sentence on stop is kept; "no free slot" retries slowly; wrong key fails once clearly. Verified with 11 fake-server tests and a real-service test using generated speech (all technical terms right, first words ~0.5s, full sentence ~2.2s after speaking stops). Standard model and current delays kept — Enhanced was no better. Connection states (never key/audio/words) log to `speechmatics-connection.log` in the app log folder. Real mic, AirPods, long calls and real network drops not yet tested.

- Live session redesigned with Apple Design principles: a subdued grey window edge, solid grey controls, separate capsule mic/speaker buttons, filled question surface and clear answer area. No resting borders on audio, question actions, lengths or alert buttons. Main app remains light.
- History moves to the bottom left; a flat joined capsule Short / Med / Long strip with 11px labels and a stable-width silver-grey Answer button stay on the right. Busy state no longer expands the button or clips the footer at 340×260. Question, answer, follow-up, errors and menus adapt to small windows. Saved opacity, text size, keyboard shortcuts and audio behavior are retained.
- Computer preview verified at 440×640 and 340×260: answer streaming/busy state, previous-answer navigation, mic off/on, search draft + Escape, options scrolling and expanded microphone errors at 40% opacity. Measured audio/length/alert controls have 0px borders; outer edge is rgb(55,55,59). Reduced-motion emulation removed animation; reduced-transparency fallback exists, but the browser did not confirm that media feature in emulation.
- 81 frontend tests pass; production build, cargo build and git diff --check pass. Scoped design detector has no findings. Latest CSS baseline /tmp/aside-capsule-correction-before.css; detailed checks in docs/evidence.md. Native session capture/cloud requests were not run for this visual change.
- Earlier changes retained: 85% Settings row spacing; light workspace; saved documents/history/logo; quicker measured readiness checks. Audio reliability remains PARTIAL: built-in mic/speakers passed in the earlier clean-start test, while AirPods startup was intermittent. Windows installer/runtime and real cloud-answer checks remain unverified.

## Next action

Claude now works (verified with real requests). Try a practice call and a screenshot answer to judge the new prompt against your Guidance. Quit and reopen Aside.app (the open copy predates the rebuild), then do one real call with Speechmatics: confirm both lanes transcribe, and if anything is rejected, check `speechmatics-connection.log` for the exact reason. Resolve intermittent native capture startup and verify a repeatable cold-start preflight on the owner's chosen microphone/output. UI work is complete; existing real-call/cloud-answer/transcript and Windows runtime checks remain.

## Rollback

Latest control CSS baseline is /tmp/aside-capsule-correction-before.css; broader live redesign baseline is /tmp/aside-live-grey-before/source.tgz. Restore only this turn's files and rebuild; preserve all pre-existing changes. No schema, key, document or history changes.
