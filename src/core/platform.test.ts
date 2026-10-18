import { test } from "node:test";
import assert from "node:assert/strict";
import { platformFrom, privacySettingsUrl, audioPermissionHelp, screenCaptureHelp } from "./platform.ts";
import { shortcutsFor } from "./shortcuts.ts";

test("Windows and Mac get the modifier registered by the native shortcuts", () => {
  assert.equal(platformFrom("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"), "windows");
  assert.equal(platformFrom("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)"), "macos");
  assert.equal(shortcutsFor("windows")[0][0], "Ctrl + Enter");
  assert.equal(shortcutsFor("windows")[2][0], "Ctrl + Shift + ,");
  assert.equal(shortcutsFor("macos")[0][0], "⌘ ↩");
});

test("Windows recovery points to desktop microphone access and speaker settings", () => {
  assert.equal(privacySettingsUrl("microphone", "windows"), "ms-settings:privacy-microphone");
  assert.equal(privacySettingsUrl("screen", "windows"), "ms-settings:sound");
  assert.match(audioPermissionHelp("mic", "windows"), /desktop apps/);
  assert.doesNotMatch(audioPermissionHelp("system", "windows"), /Screen & System Audio Recording/);
  assert.doesNotMatch(screenCaptureHelp("windows"), /System Settings/);
  assert.match(privacySettingsUrl("screen", "macos")!, /Privacy_ScreenCapture/);
  assert.equal(privacySettingsUrl("screen", "other"), null);
});
