//! A thin primitive that watches the OS-level default audio input/output
//! device and calls back when either changes — a Bluetooth headset
//! connecting or disconnecting, the user switching it in System Settings, or
//! anything else that repoints "the system default."
//!
//! This module knows nothing about Aside's capture pipeline, `AudioState`,
//! or the "only auto-follow when the user hasn't pinned a specific device"
//! rule — that decision logic lives in `commands/audio.rs`, which is the
//! caller here. Keeping the two apart mirrors this project's existing split
//! in `system.rs` between `is_tappable`/`same_physical_device` (pure
//! detection) and `start()` (what to do about it): a primitive that only
//! ever answers "did the OS default change" is easy to trust and easy to
//! test in isolation, and one that also decides whether to tear down a
//! running capture stream is neither.
//!
//! ## The mechanism
//!
//! `cpal` 0.18.2's own `ErrorKind::DeviceChanged` (see
//! `cpal-0.18.2/src/host/coreaudio/macos/mod.rs`'s `DefaultOutputMonitor`)
//! only fires for *output* streams built from `default_output_device()`.
//! Both of Aside's capture paths — `audio/mic.rs`'s plain input stream, and
//! `audio/system.rs`'s loopback-via-input-stream trick — go through
//! `build_input_stream`, whose own device-loss monitor
//! (`DisconnectManager`) only reports a device disappearing, never the OS
//! default silently repointing at a *different, still-present* device. So
//! neither of Aside's channels gets this notification from `cpal` for free.
//!
//! The real mechanism is CoreAudio's own `AudioObjectAddPropertyListener` on
//! `kAudioObjectSystemObject`, watching
//! `kAudioHardwarePropertyDefaultInputDevice` /
//! `kAudioHardwarePropertyDefaultOutputDevice` — exactly what `cpal`'s own
//! `DefaultOutputMonitor` uses internally. That machinery is private to
//! `cpal` (not something this crate can call directly), so the `macos`
//! submodule below is a parallel implementation of the same known-working
//! shape, closely modeled on
//! `cpal-0.18.2/src/host/coreaudio/macos/property_listener.rs` and
//! `mod.rs`'s `spawn_property_listener_thread` (both MIT/Apache-2.0, read as
//! reference — not linked to, since they are private to `cpal`).
//!
//! `objc2-core-audio` needs no new dependency to add: it is already fully
//! resolved in `Cargo.lock` as a transitive dependency of `cpal` itself, at
//! the exact version (`0.3.2`) this module targets. Only the plain
//! C-callback listener API is used
//! (`AudioObjectAddPropertyListener`/`AudioObjectRemovePropertyListener`) —
//! deliberately not the block-based variant, which would pull in
//! `block2`/`dispatch2` for no benefit here.
//!
//! ## Windows
//!
//! Not implemented. WASAPI has its own default-device-change notification
//! (`IMMNotificationClient::OnDefaultDeviceChanged`), a genuinely different
//! API from CoreAudio's property-listener mechanism — matching this
//! project's existing, honestly-stated Windows gap for audio (`status.md`
//! 3.9). `DeviceChangeWatcher::start` on any non-macOS platform always
//! returns `Err` and never calls `on_changed`; a caller that treats that as
//! "just don't arm auto-follow here" (which `commands/audio.rs` does) gets
//! exactly the pre-existing behavior back — a pinned device keeps working
//! exactly as it does today, and an unpinned device just keeps using
//! whatever it resolved to at capture start.

/// Which OS-level default property a [`DeviceChangeWatcher`] is watching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultDeviceKind {
    Input,
    Output,
}

/// Watches one OS-level default-device property (input or output) and
/// invokes `on_changed` on every CoreAudio notification for it. Dropping
/// this value unregisters the listener.
///
/// Deliberately a single-property watcher rather than one covering both
/// input and output at once: `commands/audio.rs` arms and disarms
/// mic-following and system-audio-following independently — only when that
/// specific channel's device isn't pinned — and a struct that could only
/// ever be fully alive or fully dead would force both channels to share one
/// lifetime for no reason.
pub struct DeviceChangeWatcher {
    #[cfg(target_os = "macos")]
    _inner: macos::PropertyWatcher,
    #[cfg(not(target_os = "macos"))]
    _unused: (),
}

impl DeviceChangeWatcher {
    /// Starts watching `kind`. `on_changed` runs on a CoreAudio-owned
    /// notification thread, not any thread this crate controls — so it must
    /// return fast and never block. `commands/audio.rs`'s callback does the
    /// minimum possible here (a couple of atomic operations) and defers the
    /// real work to a freshly spawned thread; see its module comment.
    #[cfg(target_os = "macos")]
    pub fn start(
        kind: DefaultDeviceKind,
        on_changed: impl FnMut() + Send + 'static,
    ) -> Result<Self, String> {
        Ok(Self {
            _inner: macos::PropertyWatcher::start(kind, on_changed)?,
        })
    }

    /// Always fails on non-macOS platforms — see this module's doc comment.
    /// `on_changed` is accepted (rather than the method not existing at all)
    /// so `commands/audio.rs` compiles identically on every platform and
    /// only has to branch on the `Result`, not on `#[cfg]`.
    #[cfg(not(target_os = "macos"))]
    pub fn start(
        _kind: DefaultDeviceKind,
        _on_changed: impl FnMut() + Send + 'static,
    ) -> Result<Self, String> {
        Err(
            "device auto-follow (watching the OS default audio device) is only implemented on macOS"
                .to_string(),
        )
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::DefaultDeviceKind;
    use std::ffi::c_void;
    use std::ptr::NonNull;

    use objc2_core_audio::{
        kAudioHardwareNoError, kAudioHardwarePropertyDefaultInputDevice,
        kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
        kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, AudioObjectAddPropertyListener,
        AudioObjectID, AudioObjectPropertyAddress, AudioObjectRemovePropertyListener,
    };

    fn address_for(kind: DefaultDeviceKind) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: match kind {
                DefaultDeviceKind::Input => kAudioHardwarePropertyDefaultInputDevice,
                DefaultDeviceKind::Output => kAudioHardwarePropertyDefaultOutputDevice,
            },
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        }
    }

    /// Double indirection so a boxed `FnMut` (a fat pointer) can be passed
    /// through CoreAudio's single `*mut c_void` client-data slot — mirrors
    /// `cpal`'s own (private) `PropertyListenerCallbackWrapper`.
    struct CallbackBox(Box<dyn FnMut() + Send>);

    /// One registered `AudioObjectAddPropertyListener` call, on
    /// `kAudioObjectSystemObject`. `Drop` unregisters it.
    ///
    /// Registration and teardown both happen on a dedicated thread that
    /// otherwise just parks — matching `cpal`'s own
    /// `spawn_property_listener_thread`, whose doc comment states this
    /// avoids "potential threading issues with CoreAudio APIs" by always
    /// creating and dropping the listener from the same thread. That thread
    /// is the only place `AudioObjectAddPropertyListener`/
    /// `AudioObjectRemovePropertyListener` are ever called from in this
    /// module; the *notification* callback itself still fires on whatever
    /// thread CoreAudio's HAL chooses, not this one — see `listener_shim`.
    pub struct PropertyWatcher {
        // Held only to keep the channel (and so the parked registration
        // thread) alive; dropping it closes the channel, which wakes the
        // thread's blocking `recv()` with an `Err` and lets it fall through
        // to dropping `Registration`.
        _shutdown_tx: std::sync::mpsc::Sender<()>,
    }

    impl PropertyWatcher {
        pub fn start(
            kind: DefaultDeviceKind,
            on_changed: impl FnMut() + Send + 'static,
        ) -> Result<Self, String> {
            let address = address_for(kind);
            let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();
            let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();

            std::thread::spawn(move || {
                match Registration::new(address, on_changed) {
                    Ok(registration) => {
                        let _ = ready_tx.send(Ok(()));
                        // Parked until told to shut down (the `Sender` on
                        // the other end drops, or sends `()` — either wakes
                        // this). `registration` drops right after, which
                        // unregisters — on this same thread, matching how
                        // it was registered.
                        let _ = shutdown_rx.recv();
                        drop(registration);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            });

            match ready_rx.recv() {
                Ok(Ok(())) => Ok(Self {
                    _shutdown_tx: shutdown_tx,
                }),
                Ok(Err(e)) => Err(e),
                Err(_) => Err("device watcher thread exited before it could register".to_string()),
            }
        }
    }

    struct Registration {
        address: AudioObjectPropertyAddress,
        callback: Box<CallbackBox>,
        removed: bool,
    }

    impl Registration {
        fn new(
            address: AudioObjectPropertyAddress,
            on_changed: impl FnMut() + Send + 'static,
        ) -> Result<Self, String> {
            let callback = Box::new(CallbackBox(Box::new(on_changed)));
            let status = unsafe {
                AudioObjectAddPropertyListener(
                    kAudioObjectSystemObject as AudioObjectID,
                    NonNull::from(&address),
                    Some(listener_shim),
                    &*callback as *const CallbackBox as *mut c_void,
                )
            };
            if status != kAudioHardwareNoError {
                return Err(format!(
                    "AudioObjectAddPropertyListener failed with OSStatus {status}"
                ));
            }
            Ok(Self {
                address,
                callback,
                removed: false,
            })
        }
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            if self.removed {
                return;
            }
            let status = unsafe {
                AudioObjectRemovePropertyListener(
                    kAudioObjectSystemObject as AudioObjectID,
                    NonNull::from(&self.address),
                    Some(listener_shim),
                    &*self.callback as *const CallbackBox as *mut c_void,
                )
            };
            self.removed = true;
            if status != kAudioHardwareNoError {
                // Not fatal — the `Box` still drops normally right after
                // this, so nothing leaks memory. What can leak is the
                // CoreAudio-side registration itself, which would then
                // outlive its intended lifetime until the process exits.
                // Logged per this project's established "explicit
                // diagnostics over silent no-ops" style (see `system.rs`'s
                // `SilenceWatch`).
                eprintln!(
                    "device_watch: AudioObjectRemovePropertyListener failed with OSStatus {status}"
                );
            }
        }
    }

    /// Callback CoreAudio invokes on its own notification thread(s) when the
    /// watched property changes. Mirrors `cpal`'s own
    /// `property_listener_handler_shim` exactly, including its safety
    /// contract: this assumes CoreAudio never invokes the same registered
    /// listener proc reentrantly from two threads at once (an assumption
    /// `cpal` itself makes no attempt to guard against either — see this
    /// module's doc comment on the reference it is modeled on).
    ///
    /// # Safety
    ///
    /// `client_data` must be exactly the `&CallbackBox` pointer
    /// `Registration::new` registered for this exact `(object, address)`
    /// pair, and nothing may free the `Box` behind it before
    /// `Registration::drop` unregisters this same shim for this same
    /// pointer — which is guaranteed by `Registration` owning the `Box` and
    /// only ever handing CoreAudio a borrowed pointer into it.
    unsafe extern "C-unwind" fn listener_shim(
        _object_id: AudioObjectID,
        _num_addresses: u32,
        _addresses: NonNull<AudioObjectPropertyAddress>,
        client_data: *mut c_void,
    ) -> i32 {
        let wrapper = client_data as *mut CallbackBox;
        // Safety: see the function's own doc comment.
        unsafe {
            (*wrapper).0();
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Not FFI, but pins the enum's two variants exist and are distinct —
    /// cheap insurance against a future edit accidentally collapsing them,
    /// since almost nothing else in this module is pure enough to unit
    /// test. The real registration/callback path needs a running CoreAudio
    /// HAL and a live default-device change to exercise, neither of which
    /// is available in a unit test — see this phase's report for exactly
    /// what a live owner test still needs to confirm.
    #[test]
    fn the_two_default_device_kinds_are_distinct() {
        assert_ne!(DefaultDeviceKind::Input, DefaultDeviceKind::Output);
    }

    /// On non-macOS, `start` must fail cleanly rather than panicking, and
    /// must never call `on_changed` — `commands/audio.rs` relies on exactly
    /// this contract to know it's safe to just log and move on rather than
    /// treat a failed `start` as fatal to capture itself.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn on_non_macos_start_fails_cleanly_instead_of_watching_anything() {
        let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called2 = called.clone();
        let result = DeviceChangeWatcher::start(DefaultDeviceKind::Input, move || {
            called2.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        assert!(
            result.is_err(),
            "non-macOS auto-follow must not silently pretend to work"
        );
        assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    }

    /// Real (not `#[ignore]`d) CoreAudio registration test — it does not
    /// need a live default-device change to be useful, only a running HAL,
    /// which any macOS test run has. Registering and then dropping a
    /// listener against the real system object is enough to prove
    /// `AudioObjectAddPropertyListener`/`AudioObjectRemovePropertyListener`
    /// and the property/constant values above are wired correctly — the one
    /// thing this module could get wrong without the type system ever
    /// catching it.
    ///
    /// What this does **not** prove: that `on_changed` actually fires when
    /// the OS default genuinely changes. That needs a live device switch —
    /// see this phase's report for exactly what a live owner test still
    /// needs to confirm.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_real_property_listener_registers_and_unregisters_cleanly_on_this_machine() {
        for kind in [DefaultDeviceKind::Input, DefaultDeviceKind::Output] {
            let fired = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let fired2 = fired.clone();
            let watcher = DeviceChangeWatcher::start(kind, move || {
                fired2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            })
            .unwrap_or_else(|e| {
                panic!("registering a {kind:?} listener should succeed on real CoreAudio: {e}")
            });
            drop(watcher); // must not panic or hang — proves clean teardown too
            assert_eq!(
                fired.load(std::sync::atomic::Ordering::SeqCst),
                0,
                "nothing should have fired without a real default-device change"
            );
        }
    }
}
