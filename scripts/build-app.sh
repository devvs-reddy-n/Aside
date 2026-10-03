#!/usr/bin/env bash
# Builds a real, installed-style macOS .app bundle and drops it at the
# project root as `Aside.app` — the owner's explicit, standing request (see
# CLAUDE.md) is a fresh one here after every change, specifically so macOS
# treats it as a real app for Screen & System Audio Recording / Microphone
# permissions (TCC) instead of the raw `cargo run` dev binary, whose
# ad-hoc-signed identity can change on every rebuild and silently reset
# those grants.
#
# This is a full release build — it recompiles the native ONNX Runtime
# stack in release mode, not the ~1-2s incremental dev build — so it takes
# real minutes, not seconds. That cost was surfaced to and knowingly
# accepted by the owner; see docs/decisions.md.
set -euo pipefail
cd "$(dirname "$0")/.."

# Cursor (and some CI) inject CARGO_TARGET_DIR. If we honor it, `tauri build`
# writes the bundle into that cache, then this script copies a *stale*
# `src-tauri/target/release/bundle/macos/Aside.app` and signs it as if it
# were new. Measured 2026-08-24: after the EOU cutover, the root app still
# contained `parakeet-tdt-0.6b-v2` for exactly this reason.
unset CARGO_TARGET_DIR

npm run tauri build -- --bundles app

BUNDLE="src-tauri/target/release/bundle/macos/Aside.app"
if [ ! -d "$BUNDLE" ]; then
  echo "FAIL  expected bundle not found at $BUNDLE" >&2
  exit 1
fi

rm -rf "./Aside.app"
cp -R "$BUNDLE" "./Aside.app"

# Spotlight and Raycast treat every Aside.app they can see as a separate
# launchable copy. After we have the project-root app, drop the Tauri
# intermediate so search only finds ./Aside.app. The next build recreates
# $BUNDLE. Cargo's raw debug/release executables embed the same app name and
# can be offered by Spotlight as fake extra "Aside" apps, so remove those
# disposable build products too. Also drop leftover installer images from
# older dmg-bundle runs.
rm -rf "$BUNDLE"
rm -f src-tauri/target/release/bundle/macos/*.dmg
rm -f src-tauri/target/debug/aside
rm -f src-tauri/target/release/aside
mkdir -p src-tauri/target
touch src-tauri/target/.metadata_never_index

# --- Code signing -----------------------------------------------------------
#
# `tauri build` does NOT sign the bundle unless a signing identity is
# configured, and it isn't here. What that leaves behind is worse than it
# looks: only the Mach-O carries the linker's own ad-hoc signature, so the
# *bundle* is unsigned — `Sealed Resources=none`, no `_CodeSignature`
# directory, `Info.plist` unsealed, `Identifier` set to a generated
# `aside-<hash>` rather than `com.aside.desktop`, and `codesign -v` failing
# outright.
#
# That is not cosmetic. macOS TCC stores a code requirement alongside every
# permission grant, and it was rejecting ours: the unified log shows 19
# occurrences of
#   "Failed to match existing code requirement for subject com.aside.desktop
#    and service kTCCServiceAudioCapture"
# during a single test session, i.e. the Screen & System Audio Recording grant
# was being thrown away and re-prompted on every rebuild. Signing the bundle
# properly fixes the identifier, the seal, and the validity.
#
# Being straight about the limit: ad-hoc signing does NOT make the grant
# survive a rebuild. `codesign -d -r-` on an ad-hoc bundle reports
# `designated => cdhash H"..."`, and the cdhash changes every build by
# definition. Only a stable signing certificate produces a requirement of the
# form `identifier "com.aside.desktop" and certificate leaf = H"..."`, which
# does survive. Set ASIDE_SIGN_IDENTITY to that certificate's name once one
# exists in the keychain; until then the warning below tells the truth rather
# than implying the problem is solved.
SIGN_IDENTITY="${ASIDE_SIGN_IDENTITY:--}"

# codesign refuses to touch a bundle carrying extended attributes
# ("resource fork, Finder information, or similar detritus not allowed"),
# which `cp -R` preserves. Strip them or signing never happens.
#
# There is a second-order problem specific to where this project lives. It sits
# under ~/Desktop, which is an iCloud "Desktop & Documents" synced folder, so
# macOS's file provider re-attaches `com.apple.FinderInfo` and
# `com.apple.fileprovider.fpfs#P` to the bundle root **within a couple of
# seconds** of any strip. Measured: strip, verify -> "valid on disk / satisfies
# its Designated Requirement"; wait 3s, and both xattrs are back. The signature
# is genuinely fine — `--strict` is objecting to an OS-added attribute on the
# directory, not to anything sealed. Hence: strip, sign, strip again, verify
# immediately, and retry the strip+verify once if the file provider wins the
# race. Only a failure that survives that is a real signing failure.
strip_xattrs() {
  xattr -cr "./Aside.app" 2>/dev/null || true
  xattr -c  "./Aside.app" 2>/dev/null || true
}

# iCloud Desktop can re-attach FinderInfo between strip and codesign.
# Retry a few times instead of failing on that race.
signed=0
for _ in 1 2 3 4 5 6; do
  strip_xattrs
  if codesign --force --sign "$SIGN_IDENTITY" --identifier com.aside.desktop "./Aside.app"; then
    signed=1
    break
  fi
  sleep 0.25
done
if [ "$signed" -ne 1 ]; then
  echo "FAIL  codesign could not sign ./Aside.app after retries" >&2
  exit 1
fi

# iCloud may restore FinderInfo for several seconds after the bundle is copied.
# Keep strict verification; only retry the known metadata race for a bounded time.
verified=0
for attempt in 1 2 3 4 5 6; do
  xattr -d com.apple.FinderInfo "./Aside.app" 2>/dev/null || true
  if codesign --verify --strict --verbose=2 "./Aside.app"; then
    verified=1
    break
  fi
  if [ "$attempt" -lt 6 ]; then
    echo "NOTE  strict verification failed; retrying after iCloud metadata settles" >&2
    sleep 2
  fi
done
if [ "$verified" -ne 1 ]; then
  echo "FAIL  strict signature verification failed after retries" >&2
  exit 1
fi

ACTUAL_ID="$(codesign -dvvv "./Aside.app" 2>&1 | sed -n 's/^Identifier=//p')"
if [ "$ACTUAL_ID" != "com.aside.desktop" ]; then
  echo "FAIL  signed bundle reports Identifier=$ACTUAL_ID, expected com.aside.desktop" >&2
  exit 1
fi

if [ "$SIGN_IDENTITY" = "-" ]; then
  echo
  echo "NOTE  Signed ad-hoc. The bundle is now valid and correctly identified,"
  echo "      but its designated requirement is a cdhash, which changes every"
  echo "      build — so macOS will ask again for Screen & System Audio"
  echo "      Recording and Microphone permission after this rebuild."
  echo "      To stop that: create a self-signed code-signing certificate and"
  echo "      re-run with ASIDE_SIGN_IDENTITY=\"<certificate name>\"."
  echo
fi

echo "DONE  ./Aside.app refreshed from $BUNDLE (signed as $ACTUAL_ID)"
