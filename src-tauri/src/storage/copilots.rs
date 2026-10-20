use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::llm::LlmProviderKind;

/// Interview setups and a retired screen marker retained to read older databases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopilotKind {
    #[default]
    Interview,
    Screen,
}

impl CopilotKind {
    fn as_db(self) -> &'static str {
        match self {
            CopilotKind::Interview => "interview",
            CopilotKind::Screen => "screen",
        }
    }

    fn from_db(value: &str) -> Self {
        if value == "screen" {
            CopilotKind::Screen
        } else {
            CopilotKind::Interview
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Copilot {
    pub id: Uuid,
    pub name: String,
    pub company: Option<String>,
    pub target_role: Option<String>,
    pub job_description: Option<String>,
    pub instructions: Option<String>,
    pub save_transcript: bool,
    pub llm_provider: LlmProviderKind,
    pub llm_model: String,
    #[serde(default)]
    pub kind: CopilotKind,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CopilotSummary {
    #[serde(flatten)]
    pub copilot: Copilot,
    pub document_count: usize,
    pub has_resume: bool,
}

fn provider_from_db(value: &str) -> Result<LlmProviderKind, String> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .map_err(|_| format!("unknown Copilot provider: {value}"))
}

fn row_to_copilot(row: &rusqlite::Row) -> rusqlite::Result<Copilot> {
    let id: String = row.get("id")?;
    let provider: String = row.get("llm_provider")?;
    let provider = provider_from_db(&provider).map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(Copilot {
        id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
        name: row.get("name")?,
        company: row.get("company")?,
        target_role: row.get("target_role")?,
        job_description: row.get("job_description")?,
        instructions: row.get("instructions")?,
        save_transcript: row.get::<_, i64>("save_transcript")? != 0,
        llm_provider: provider,
        llm_model: row.get("llm_model")?,
        kind: CopilotKind::from_db(&row.get::<_, String>("kind")?),
        created_at_ms: row.get("created_at_ms")?,
        updated_at_ms: row.get("updated_at_ms")?,
    })
}

pub fn create(conn: &Connection, copilot: &Copilot) -> Result<(), String> {
    let provider = serde_json::to_value(copilot.llm_provider)
        .map_err(|e| format!("failed to serialize Copilot provider: {e}"))?
        .as_str()
        .unwrap_or_default()
        .to_owned();
    conn.execute(
        "INSERT INTO copilots (id, name, company, target_role, job_description, instructions, save_transcript, llm_provider, llm_model, created_at_ms, updated_at_ms, kind)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            copilot.id.to_string(), copilot.name, copilot.company, copilot.target_role,
            copilot.job_description, copilot.instructions,
            copilot.save_transcript as i64, provider, copilot.llm_model,
            copilot.created_at_ms, copilot.updated_at_ms, copilot.kind.as_db(),
        ],
    ).map_err(|e| format!("failed to create Copilot: {e}"))?;
    Ok(())
}

pub fn update(conn: &Connection, copilot: &Copilot) -> Result<bool, String> {
    let provider = serde_json::to_value(copilot.llm_provider)
        .map_err(|e| format!("failed to serialize Copilot provider: {e}"))?
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let changed = conn.execute(
        "UPDATE copilots SET name=?1, company=?2, target_role=?3, job_description=?4, instructions=?5,
          save_transcript=?6, llm_provider=?7, llm_model=?8, updated_at_ms=?9, kind=?10 WHERE id=?11",
        params![
            copilot.name, copilot.company, copilot.target_role, copilot.job_description,
            copilot.instructions, copilot.save_transcript as i64,
            provider, copilot.llm_model, copilot.updated_at_ms, copilot.kind.as_db(), copilot.id.to_string(),
        ],
    ).map_err(|e| format!("failed to save Copilot: {e}"))?;
    Ok(changed > 0)
}

pub fn get(conn: &Connection, id: Uuid) -> Result<Option<Copilot>, String> {
    conn.query_row(
        "SELECT * FROM copilots WHERE id=?1",
        params![id.to_string()],
        row_to_copilot,
    )
    .optional()
    .map_err(|e| format!("failed to load Copilot: {e}"))
}

pub fn list(conn: &Connection) -> Result<Vec<CopilotSummary>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT c.*, COUNT(cd.document_id) AS document_count, MAX(cd.is_resume) AS has_resume
         FROM copilots c LEFT JOIN copilot_documents cd ON cd.copilot_id=c.id
         GROUP BY c.id ORDER BY c.updated_at_ms DESC",
        )
        .map_err(|e| format!("failed to prepare Copilot list: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(CopilotSummary {
                copilot: row_to_copilot(row)?,
                document_count: row.get::<_, i64>("document_count")?.max(0) as usize,
                has_resume: row.get::<_, Option<i64>>("has_resume")?.unwrap_or(0) != 0,
            })
        })
        .map_err(|e| format!("failed to load Copilots: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read Copilots: {e}"))
}

pub fn delete(conn: &Connection, id: Uuid) -> Result<bool, String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let deleted = tx
        .execute("DELETE FROM copilots WHERE id=?1", params![id.to_string()])
        .map_err(|e| format!("failed to delete Copilot: {e}"))?
        > 0;
    // Display summaries belong to their copilot, including their cached source text.
    for kind in ["job_description", "guidance"] {
        super::settings::delete(&tx, &format!("setup_summary:{id}:{kind}"))?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(deleted)
}

pub fn replace_documents(
    conn: &Connection,
    copilot_id: Uuid,
    resume_id: Option<Uuid>,
    document_ids: &[Uuid],
    now_ms: u64,
) -> Result<(), String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("failed to begin Copilot source update: {e}"))?;
    tx.execute(
        "DELETE FROM copilot_documents WHERE copilot_id=?1",
        params![copilot_id.to_string()],
    )
    .map_err(|e| format!("failed to clear Copilot sources: {e}"))?;
    if let Some(id) = resume_id {
        tx.execute("INSERT INTO copilot_documents (copilot_id, document_id, is_resume, attached_at_ms) VALUES (?1, ?2, 1, ?3)",
            params![copilot_id.to_string(), id.to_string(), now_ms])
            .map_err(|e| format!("failed to attach Copilot resume: {e}"))?;
    }
    for id in document_ids.iter().filter(|id| Some(**id) != resume_id) {
        tx.execute("INSERT INTO copilot_documents (copilot_id, document_id, is_resume, attached_at_ms) VALUES (?1, ?2, 0, ?3)",
            params![copilot_id.to_string(), id.to_string(), now_ms])
            .map_err(|e| format!("failed to attach Copilot document: {e}"))?;
    }
    tx.commit()
        .map_err(|e| format!("failed to save Copilot sources: {e}"))
}

pub fn document_ids(conn: &Connection, copilot_id: Uuid) -> Result<Vec<Uuid>, String> {
    let mut stmt = conn.prepare("SELECT document_id FROM copilot_documents WHERE copilot_id=?1 ORDER BY is_resume DESC, attached_at_ms ASC")
        .map_err(|e| format!("failed to prepare Copilot sources: {e}"))?;
    let rows = stmt
        .query_map(params![copilot_id.to_string()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| format!("failed to load Copilot sources: {e}"))?;
    rows.map(|id| {
        id.map_err(|e| e.to_string())
            .and_then(|id| Uuid::parse_str(&id).map_err(|e| e.to_string()))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Db;

    fn sample(now: u64) -> Copilot {
        Copilot {
            id: Uuid::new_v4(),
            name: "Stripe platform role".into(),
            company: Some("Stripe".into()),
            target_role: Some("Platform engineer".into()),
            job_description: None,
            instructions: None,
            save_transcript: true,
            llm_provider: LlmProviderKind::OpenAi,
            llm_model: "gpt-6-luna".into(),
            kind: CopilotKind::Interview,
            created_at_ms: now,
            updated_at_ms: now,
        }
    }

    #[test]
    fn deleting_copilot_also_removes_only_its_display_summaries() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let copilot = sample(100);
        create(&conn, &copilot).unwrap();
        let key = format!("setup_summary:{}:guidance", copilot.id);
        super::super::settings::set(&conn, &key, "cached source").unwrap();
        super::super::settings::set(&conn, "unrelated", "keep").unwrap();
        assert!(delete(&conn, copilot.id).unwrap());
        assert!(super::super::settings::get(&conn, &key).unwrap().is_none());
        assert_eq!(super::super::settings::get(&conn, "unrelated").unwrap().as_deref(), Some("keep"));
    }

    #[test]
    fn create_list_and_update_round_trip() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let mut copilot = sample(100);
        create(&conn, &copilot).unwrap();
        assert_eq!(list(&conn).unwrap()[0].copilot.name, "Stripe platform role");
        copilot.name = "Payments platform role".into();
        copilot.updated_at_ms = 200;
        assert!(update(&conn, &copilot).unwrap());
        assert_eq!(
            get(&conn, copilot.id).unwrap().unwrap().name,
            "Payments platform role"
        );
    }

    #[test]
    fn a_recruiter_screen_keeps_its_kind() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let mut copilot = sample(100);
        copilot.kind = CopilotKind::Screen;
        create(&conn, &copilot).unwrap();
        assert_eq!(get(&conn, copilot.id).unwrap().unwrap().kind, CopilotKind::Screen);
        copilot.kind = CopilotKind::Interview;
        update(&conn, &copilot).unwrap();
        assert_eq!(list(&conn).unwrap()[0].copilot.kind, CopilotKind::Interview);
    }

    #[test]
    fn one_resume_and_many_reusable_sources_are_saved_per_copilot() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        let copilot = sample(100);
        create(&conn, &copilot).unwrap();
        let resume = crate::storage::documents::insert_document(
            &conn,
            None,
            "resume",
            "resume.pdf",
            "resume text",
            1,
        )
        .unwrap();
        let note = crate::storage::documents::insert_document(
            &conn,
            None,
            "other",
            "brief.pdf",
            "brief text",
            2,
        )
        .unwrap();
        replace_documents(&conn, copilot.id, Some(resume), &[note], 200).unwrap();
        assert_eq!(document_ids(&conn, copilot.id).unwrap(), vec![resume, note]);
        let summary = list(&conn).unwrap().pop().unwrap();
        assert!(summary.has_resume);
        assert_eq!(summary.document_count, 2);
    }
}
