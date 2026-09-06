//! Tiny helpers shared by both capture paths (`mic.rs`, `system.rs`) for
//! naming and identifying a `cpal::Device`, plus the device-*identity*
//! logic (`same_physical_device`/`strip_device_role_suffix`) shared by
//! `system.rs`'s AirPods-tap-collision detector and `device_watch.rs`'s
//! same-physical-device restart suppression.
//!
//! Factored out rather than duplicated per `channel.rs`'s documented lesson
//! (see its module doc comment): Natively ended up with two near-identical
//! audio loops that drifted apart because nothing forced a fix made to one to
//! reach the other. `device_name`/`device_id` are small, but they are exactly
//! the "same logic, two call sites" shape that lesson is about — `system.rs`
//! had both already; `mic.rs` needed them for its own device picker and would
//! otherwise have grown a second, easy-to-desync copy.
//!
//! `same_physical_device`/`strip_device_role_suffix` moved here from
//! `system.rs` for the same reason, once the device-auto-follow feature
//! needed the identical "is this really the same hardware" check to decide
//! whether an OS default-device change notification is worth acting on —
//! see `commands/audio.rs`'s restart-suppression guard.

use cpal::traits::{DeviceTrait, HostTrait};

/// The device's plain name, for display.
///
/// Deliberately `description().name()` and not `description().to_string()`:
/// `Display` decorates the name with manufacturer, form factor and interface
/// type, so on Windows it renders as e.g.
/// `Speakers (Realtek(R) Audio) [Speakers] [PCI]`.
pub fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "unknown device".to_string())
}

/// The device's stable identifier, for saving and re-resolving a choice.
///
/// Names are neither unique nor stable — two identically-named devices are
/// ordinary, and a driver update can change the decorated form. cpal 0.17
/// added real device ids for exactly this; matching on them means a saved
/// preference survives a rename.
pub fn device_id(device: &cpal::Device) -> Option<String> {
    device.id().ok().map(|id| id.id().to_string())
}

/// The OS's current default input device's stable id, if any.
///
/// A thin wrapper over `cpal::Host`, re-resolved fresh on every call rather
/// than cached — exactly what a caller reacting to "the default just
/// changed" needs. Like `mic::resolve_device`/`list_input_devices`, there is
/// no pure sub-piece to isolate here without inventing a mockable host trait
/// purely for this; see `mic.rs`'s own note on that trade-off.
pub fn default_input_device_id() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .and_then(|d| device_id(&d))
}

/// The OS's current default output device's stable id, if any.
pub fn default_output_device_id() -> Option<String> {
    cpal::default_host()
        .default_output_device()
        .and_then(|d| device_id(&d))
}

/// The OS's current default input device's display name, if any. Used only
/// for a human-readable notice ("switched to X") — `default_input_device_id`
/// is what identity decisions are actually made on.
pub fn default_input_device_name() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .map(|d| device_name(&d))
}

/// The OS's current default output device's display name, if any.
pub fn default_output_device_name() -> Option<String> {
    cpal::default_host()
        .default_output_device()
        .map(|d| device_name(&d))
}

/// Drops CoreAudio's `:input` / `:output` role suffix from a device id.
///
/// A single physical Bluetooth headset is presented by macOS as **two**
/// CoreAudio devices sharing one hardware id — verified on this machine,
/// where the default input is `38-C4-3A-5A-28-F0:input` and the default
/// output is `38-C4-3A-5A-28-F0:output`. That is why `system.rs`'s
/// `is_tappable` cannot catch the tap-collision case on its own: the
/// *output* object genuinely has no input side, so `supports_input()` is
/// false and that guard alone passes it as tappable.
///
/// Case-insensitive on the suffix only; the id itself is returned untouched
/// otherwise, since ids are opaque and comparing them is the only valid use.
pub fn strip_device_role_suffix(id: &str) -> &str {
    for suffix in [":input", ":output"] {
        if id.len() > suffix.len() {
            let (head, tail) = id.split_at(id.len() - suffix.len());
            if tail.eq_ignore_ascii_case(suffix) {
                return head;
            }
        }
    }
    id
}

/// True when two device ids name the same piece of hardware.
///
/// Originally built (see `system.rs`'s use at stream start) to detect the
/// documented CoreAudio limitation that a process tap on a device which is
/// *simultaneously the active microphone* starts successfully and then
/// delivers nothing but zeros — no error, no callback failure, just
/// silence. `references/Repo1-Natively` hit this in production and ships
/// the same suffix-stripping comparison for it. `device_watch`'s
/// restart-suppression guard reuses the identical check for a different
/// purpose: telling a
/// genuine default-device change apart from a spurious re-announce of the
/// device already in use, whose id happens to carry a different role
/// suffix.
///
/// `None` on either side is **not** a match: an unknown id is unknown, and
/// guessing here would produce a false positive (a tap-collision warning
/// naming a device we cannot identify, or a restart skipped when it
/// genuinely should have run).
pub fn same_physical_device(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => strip_device_role_suffix(a) == strip_device_role_suffix(b),
        _ => false,
    }
}

/// Exact owner-facing copy. Laptop speakers play into the open mic.
pub const SPEAKER_ECHO_NOTICE: &str =
    "Laptop speakers play into the room. Aside now ignores that copy on Me; headphones are still best.";

/// True when this output is the laptop's open speakers, not headphones.
///
/// `BuiltInSpeakerDevice` and `BuiltInMicrophoneDevice` are **different**
/// hardware (`same_physical_device` is correctly false). The leak is
/// acoustic: the speakers play into the mic. Headphones/AirPods do not
/// match.
pub fn is_open_loudspeaker(id: Option<&str>, name: Option<&str>) -> bool {
    let id = id.unwrap_or("").to_ascii_lowercase();
    let name = name.unwrap_or("").to_ascii_lowercase();
    if id.contains("airpods") || name.contains("airpods") || name.contains("headphone") {
        return false;
    }
    if id.contains("builtinspeaker") {
        return true;
    }
    if name.contains("built-in speaker") || name.contains("internal speaker") {
        return true;
    }
    name.contains("macbook") && name.contains("speaker")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bluetooth_headsets_two_sides_are_recognised_as_one_device() {
        // The literal ids this machine reports (see `system.rs`'s
        // `print_this_machines_real_device_ids`) — a real capture of the
        // configuration that produced the zero-signal bug, not invented
        // strings.
        assert!(same_physical_device(
            Some("38-C4-3A-5A-28-F0:output"),
            Some("38-C4-3A-5A-28-F0:input")
        ));
    }

    #[test]
    fn the_built_in_speakers_and_microphone_are_not_the_same_device() {
        // The regression that matters in the other direction: this exact
        // pairing was confirmed live to capture system audio perfectly with
        // the mic open, so warning about it (or suppressing a genuine
        // restart because of it) would be a false alarm on a working setup.
        assert!(!same_physical_device(
            Some("BuiltInSpeakerDevice"),
            Some("BuiltInMicrophoneDevice")
        ));
    }

    #[test]
    fn built_in_speakers_are_open_loudspeakers_and_airpods_are_not() {
        assert!(is_open_loudspeaker(
            Some("BuiltInSpeakerDevice:output"),
            Some("MacBook Pro Speakers")
        ));
        assert!(is_open_loudspeaker(Some("BuiltInSpeakerDevice"), None));
        assert!(!is_open_loudspeaker(
            Some("38-C4-3A-5A-28-F0:output"),
            Some("Devendra's AirPods Pro")
        ));
        assert!(!is_open_loudspeaker(
            Some("BuiltInMicrophoneDevice"),
            Some("MacBook Pro Microphone")
        ));
        assert!(!is_open_loudspeaker(None, Some("External Headphones")));
        assert_eq!(
            SPEAKER_ECHO_NOTICE,
            "Laptop speakers play into the room. Aside now ignores that copy on Me; headphones are still best."
        );
    }

    #[test]
    fn an_unknown_id_on_either_side_is_never_a_match() {
        // Guessing would produce a false positive on either caller: a
        // tap-collision warning naming a device we cannot identify, or a
        // device-watch restart silently skipped when it should have run.
        assert!(!same_physical_device(None, Some("38-C4-3A-5A-28-F0:input")));
        assert!(!same_physical_device(
            Some("38-C4-3A-5A-28-F0:output"),
            None
        ));
        assert!(!same_physical_device(None, None));
    }

    #[test]
    fn only_the_exact_role_suffix_is_stripped() {
        assert_eq!(strip_device_role_suffix("abc:output"), "abc");
        assert_eq!(strip_device_role_suffix("abc:input"), "abc");
        // Not a role suffix — stripping it would merge genuinely different
        // devices whose ids merely contain the word.
        assert_eq!(strip_device_role_suffix("abc:inputs"), "abc:inputs");
        assert_eq!(strip_device_role_suffix("abc-input"), "abc-input");
        assert_eq!(
            strip_device_role_suffix("MSLoopbackDriverDevice_UID"),
            "MSLoopbackDriverDevice_UID"
        );
    }

    #[test]
    fn a_bare_role_suffix_is_left_alone_rather_than_collapsing_to_empty() {
        // Without the length guard both of these would strip to "" and then
        // compare equal — every unnamed device matching every other one.
        assert_eq!(strip_device_role_suffix(":input"), ":input");
        assert_eq!(strip_device_role_suffix(":output"), ":output");
        assert!(!same_physical_device(Some(":input"), Some(":output")));
    }

    #[test]
    fn the_suffix_match_is_case_insensitive() {
        assert_eq!(strip_device_role_suffix("abc:OUTPUT"), "abc");
        assert!(same_physical_device(Some("abc:Output"), Some("abc:Input")));
    }
}
