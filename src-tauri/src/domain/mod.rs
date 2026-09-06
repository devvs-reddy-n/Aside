pub mod types;
pub mod validate;

pub use types::{Session, SessionStatus, SessionType};
// `SessionError` is deliberately not re-exported here: every caller so far
// just stringifies it for the Tauri boundary, so re-exporting it only produced
// an unused-import warning. It's still reachable as `domain::validate::SessionError`
// for the first caller that actually needs to match on a variant.
pub use validate::{validate_new_session, validate_transition};
