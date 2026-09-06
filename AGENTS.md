Everything this AI is doing and has built must be explained in simple vocabulary that's easily understandable to someone without a technical background.

Every feature that gets built must be evaluated against `references/Repo1-Natively` and `references/Repo2-OpenCluely`. Check how they approached the same problem, and if their implementation is better, build Aside's version in that style. Adapt ideas and patterns — don't copy code verbatim; both are a different tech stack (Electron/JS) from Aside's (Rust/Tauri), and Natively's license restricts reuse of its actual source.


## Startup sequence

1. Read `CONTEXT.md` and `CURRENT.md`.
2. Check `git status`, `git diff`, and recent `git log`.
3. Use judgment from there. Keep `cargo build` (in `src-tauri/`) and `npm run build` green.

## App bundle after every change

Owner-requested standing rule: after every change, run `./scripts/build-app.sh` and leave the result at the project root as `Aside.app`. This gives a real, installed-style macOS app (its own bundle identity for Screen & System Audio Recording / Microphone permissions) instead of the raw `cargo run` dev binary.

## Quality bar

For any material change: a short plan (what's affected, design, risks, rollback, how it'll be tested) before writing code — proportionate to the size of the change, not a form to fill out for a one-line fix. Review the actual diff and behavior before calling something done, not just that it compiles. Label status truthfully: **BUILT + VERIFIED**, **PARTIAL**, **DESIGNED — UNBUILT**, or **TBD** — never claim more than what's actually been run and checked.

## Files

- `CONTEXT.md` — what Aside is, architecture, non-goals. Stable; update only when a fact changes.
- `CURRENT.md` — current objective, verified state, and next action. Rewrite as state changes.
- `status.md` — granular roadmap checklist (done / partial / not started), one line per feature. Rewrite items' state as work happens; `CURRENT.md` stays the high-level snapshot, this is the itemized companion.
- `docs/decisions.md` — short, append-only log of real decisions.
- `docs/evidence.md` — commands actually run and their results. Never record a planned check as done.
- `docs/requirements.md` — only if requirements outgrow `CONTEXT.md`'s budget.

No other standing docs, no project wiki, no weekly summaries. This is a personal project for two or three people — keep the process proportionate to that, not to a company.
