//! Cached default context (resume, job, notes) for Command+Enter.
//!
//! Stable prefix is loaded at session start and on document/instruction
//! changes. The hot path reads this snapshot instead of querying SQLite
//! again on every press.

use uuid::Uuid;

use super::prompt::DefaultContext;

#[derive(Debug, Clone, Default)]
pub struct DefaultsCache {
    session_id: Option<Uuid>,
    ctx: DefaultContext,
    valid: bool,
}

impl DefaultsCache {
    pub fn get(&self, session_id: Option<Uuid>) -> Option<&DefaultContext> {
        if self.valid && self.session_id == session_id {
            Some(&self.ctx)
        } else {
            None
        }
    }

    pub fn store(&mut self, session_id: Option<Uuid>, ctx: DefaultContext) {
        self.session_id = session_id;
        self.ctx = ctx;
        self.valid = true;
    }

    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    #[cfg(test)]
    pub fn is_valid(&self) -> bool {
        self.valid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_store_then_get_for_the_same_session_is_a_hit() {
        let mut cache = DefaultsCache::default();
        let sid = Uuid::nil();
        let mut ctx = DefaultContext::default();
        ctx.resume = Some("I built payments.".into());
        cache.store(Some(sid), ctx.clone());
        let hit = cache.get(Some(sid)).expect("warm cache");
        assert_eq!(hit.resume.as_deref(), Some("I built payments."));
        assert!(cache.is_valid());
    }

    #[test]
    fn a_different_session_is_a_miss() {
        let mut cache = DefaultsCache::default();
        cache.store(Some(Uuid::nil()), DefaultContext::default());
        assert!(cache.get(Some(Uuid::new_v4())).is_none());
    }

    #[test]
    fn invalidate_forces_the_next_press_to_reload() {
        let mut cache = DefaultsCache::default();
        cache.store(Some(Uuid::nil()), DefaultContext::default());
        cache.invalidate();
        assert!(cache.get(Some(Uuid::nil())).is_none());
        assert!(!cache.is_valid());
    }

    #[test]
    fn replacing_the_resume_after_invalidate_is_what_the_next_get_returns() {
        let mut cache = DefaultsCache::default();
        let sid = Uuid::nil();
        let mut first = DefaultContext::default();
        first.resume = Some("old resume".into());
        cache.store(Some(sid), first);
        cache.invalidate();
        let mut second = DefaultContext::default();
        second.resume = Some("new resume".into());
        cache.store(Some(sid), second);
        assert_eq!(
            cache.get(Some(sid)).unwrap().resume.as_deref(),
            Some("new resume")
        );
    }
}
