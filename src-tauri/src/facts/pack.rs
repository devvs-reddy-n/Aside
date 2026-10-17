//! Heuristic knowledge pack from a resume or job description.
//! No extra LLM. Document facts land at priority 50.

use crate::facts::extract;
use crate::storage::facts::{self, PRIORITY_DOCUMENT, SOURCE_DOCUMENT};
use crate::storage::knowledge::{
    self, KIND_COMPANY_TERM, KIND_JD_LINE, KIND_SKILL, KIND_STT_GLOSSARY, KIND_TECH,
};
use rusqlite::Connection;
use uuid::Uuid;


pub fn ingest_document_pack(
    conn: &Connection,
    session_id: Uuid,
    text: &str,
    company: Option<&str>,
    now_ms: u64,
) -> Result<(), String> {
    for fact in extract::extract(text) {
        facts::upsert_fact_ranked(
            conn,
            session_id,
            &fact.key,
            &fact.value,
            SOURCE_DOCUMENT,
            PRIORITY_DOCUMENT,
            None,
            Some(fact.confidence),
            now_ms,
        )?;
    }

    if let Some(c) = company.map(str::trim).filter(|s| !s.is_empty()) {
        knowledge::insert(conn, session_id, KIND_COMPANY_TERM, c, now_ms)?;
        knowledge::insert(conn, session_id, KIND_STT_GLOSSARY, c, now_ms)?;
    }

    for tech in crate::stt::vocabulary::extract(None, Some(text)) {
        knowledge::insert(conn, session_id, KIND_TECH, &tech, now_ms)?;
        knowledge::insert(conn, session_id, KIND_STT_GLOSSARY, &tech, now_ms)?;
    }

    for line in text.lines() {
        let line = line.trim();
        if line.len() < 12 {
            continue;
        }
        let lower = line.to_lowercase();
        if lower.contains("experience") || lower.contains("proficient") || lower.contains("skilled")
        {
            knowledge::insert(conn, session_id, KIND_SKILL, &clip(line, 120), now_ms)?;
        }
    }

    let mut jd_lines = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.len() < 20 {
            continue;
        }
        knowledge::insert(conn, session_id, KIND_JD_LINE, &clip(line, 200), now_ms)?;
        jd_lines += 1;
        if jd_lines >= 8 {
            break;
        }
    }


    Ok(())
}

fn clip(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        s.to_string()
    } else {
        s.chars().take(cap).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Session;
    use crate::storage::{facts as fact_store, sessions, Db};

    #[test]
    fn a_resume_h1b_line_is_a_document_fact_at_priority_50() {
        let db = Db::open_in_memory();
        let session = Session::new("t", 1);
        let conn = db.0.lock().unwrap();
        sessions::create(&conn, &session).unwrap();
        ingest_document_pack(
            &conn,
            session.id,
            "My visa status is H-1B. I use Rust and AWS daily.",
            Some("Acme"),
            1_000,
        )
        .unwrap();
        let visa = fact_store::get(&conn, session.id, "visa_status")
            .unwrap()
            .unwrap();
        assert_eq!(visa.value, "H-1B");
        assert_eq!(visa.priority, PRIORITY_DOCUMENT);
        let glossary = crate::storage::knowledge::glossary(
            &conn,
            session.id,
            crate::storage::knowledge::GLOSSARY_CAP,
        )
        .unwrap();
        assert!(glossary.iter().any(|t| t == "Acme"));
        assert!(glossary.iter().any(|t| t == "Rust"));
    }
}
