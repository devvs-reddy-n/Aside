use thiserror::Error;

use super::types::{Session, SessionStatus};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SessionError {
    #[error("session title must not be empty")]
    EmptyTitle,
    #[error("cannot transition an ended session back to active")]
    ReactivatedEndedSession,
}

pub fn validate_new_session(session: &Session) -> Result<(), SessionError> {
    if session.title.trim().is_empty() {
        return Err(SessionError::EmptyTitle);
    }
    Ok(())
}

pub fn validate_transition(from: SessionStatus, to: SessionStatus) -> Result<(), SessionError> {
    if from == SessionStatus::Ended && to == SessionStatus::Active {
        return Err(SessionError::ReactivatedEndedSession);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_title() {
        let session = Session::new("", 0);
        assert_eq!(
            validate_new_session(&session),
            Err(SessionError::EmptyTitle)
        );
    }

    #[test]
    fn accepts_real_title() {
        let session = Session::new("1:1 with Sam", 0);
        assert!(validate_new_session(&session).is_ok());
    }

    #[test]
    fn rejects_reactivating_an_ended_session() {
        assert_eq!(
            validate_transition(SessionStatus::Ended, SessionStatus::Active),
            Err(SessionError::ReactivatedEndedSession)
        );
    }

    #[test]
    fn allows_draft_to_active() {
        assert!(validate_transition(SessionStatus::Draft, SessionStatus::Active).is_ok());
    }
}
