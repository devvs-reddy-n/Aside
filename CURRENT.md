# Current status

**BUILT + VERIFIED: opaque vertical workspace and native Mac editor navigation.**

## Latest refinement, 2026-09-26

Copilot and Settings content panels now share a narrower 752px maximum width. Copilot's five section explanations live in clickable, keyboard-accessible info buttons beside their headings; guidance keeps its Answer rules link there. Import errors remain visible inline. Description, Guidance, job URL, Strict rules preview and full text editors have an opaque gray background in both themes. Browser checked: all five info buttons show correct content, Escape closes them, and both page/scroll heights remain 792/792 at 1180×860. No saved data changed.

## Earlier typography refinement, 2026-09-26

Copilot headings and Edit/Manage actions now sit above their panels. Description/Guidance and Strict rules previews use readable secondary gray; full text editors use the same softer color. Copilot headings are 14px, previews 13.5px, Settings labels 14px, rules preview 13px and full editors 15px. Settings shortcuts now form one vertical list at 12px. Both Copilot and Settings browser layouts fit the 1180×860 window (page/scroll 792/792). Full rules editor still opens after the warning. No saved content or behavior changed.

## Previous change, 2026-09-26

Copilot stacks five separate polished tiles vertically; removed Job/Resume/Audio readiness chips. Description and Guidance show five left-aligned lines with Edit only. Opaque sidebar and content panes have different neutral shades and raised surfaces. Settings order: Appearance, Hide from screen sharing, AI model, Speech, small plain-text shortcuts, Strict rules. Rules Edit first warns: “Changing strict rules affects every generated answer.”

Main window resizing/maximizing is disabled at 1180×860; custom Zoom/fullscreen actions cannot resize main. Live resizing remains unchanged. History shortcuts now use Command+Shift+Comma/Period (Command+< / >). Command+Left/Right are no longer registered by Aside. Shared Radix warning dialogs portal above editors/menus and request native focus.

## Verified

- `npm run build`, `cargo build`, 65 frontend tests, 16 shortcut tests and 5 lifecycle tests passed. Shortcut tests reject the old arrow combinations. Design detector returned no findings; diff check passed.
- Light/dark browser at 1180×860: vertical Settings order, plain shortcut text, rules warning, full editor, and reset confirmation above the editor. Copilot with a long title fits with page/scroll height 792/792; all tile left edges match. Description and Guidance previews are 90px, five 18px lines, with zero expand controls. No browser errors.
- Final packaged Mac app relaunched from idle: opaque panes and five-line saved-content previews visually verified. Zoom button disabled. Command+Right followed by Command+Shift+Left selected the current line in the real editor; Command+Left remained available. No saved text changed. Native rules warning appeared in front and dismissed correctly.
- Final `./scripts/build-app.sh` passed; root Aside.app refreshed and signature checked (`/tmp/aside-opaque-final-build.log`). Windows appearance and actual global history navigation during a live call were not tested.

## Retained behavior

Only Command+Enter (Answer), Command+Shift+Enter (Screenshot answer), Command+< (previous answer) and Command+> (next answer) remain as app shortcuts. Standard text editing remains. Answer chooses length automatically; Short/Medium/Long immediately request that length. Without new speech, Answer reuses the last speech window. Answer shows one question/answer with history navigation; Chat shows the conversation. Type opens a composer above bottom controls. End starts a four-second cancel window then finishes automatically. Silence is normal; microphone/system icons glow independently from audio signals. Capture errors retain recovery help. Live minimum size is 340×260.

## Remaining real-device checks

Global shortcuts while another app has focus; microphone/system audio; screen capture through the configured AI provider; actual model response lengths; native resize dragging and screen-share hiding. Browser demo checks do not establish these outcomes. No real screenshot was sent to a provider during this change.

Changes remain uncommitted on `redesign-dock`. Neither named comparison repository exists. Compared the available Assistant reference’s native-vibrancy/fallback pattern; no source copied. Exceptional narrow panes may scroll to preserve access; no-scroll applies to the supported desktop overview, not unlimited text in its reading panel.
