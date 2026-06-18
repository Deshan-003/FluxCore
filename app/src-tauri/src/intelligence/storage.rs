use super::models::*;
use crate::database::Database;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};

fn encode(value: &impl Serialize) -> rusqlite::Result<String> {
    serde_json::to_string(value).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

fn decode<T: DeserializeOwned>(value: String) -> rusqlite::Result<T> {
    serde_json::from_str(&value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

pub fn policy(db: &Database) -> Result<Policy, String> {
    let policy = db
        .with_connection(|conn| {
            conn.query_row(
                "SELECT payload FROM intelligence_policy WHERE id=1",
                [],
                |r| decode(r.get(0)?),
            )
            .optional()
        })?
        .unwrap_or_default();
    Policy::validate(&policy)?;
    Ok(policy)
}

pub fn configure(db: &Database, policy: &Policy, closed: Option<&Action>) -> Result<(), String> {
    db.with_connection(|conn| {
        let tx = conn.unchecked_transaction()?;
        tx.execute("INSERT INTO intelligence_policy VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload", [encode(policy)?])?;
        if let Some(action) = closed { record(&tx, "action", &action.id, action.at, action)?; }
        tx.commit()
    })
}

pub fn record(
    conn: &Connection,
    kind: &str,
    id: &str,
    at: i64,
    value: &impl Serialize,
) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO intelligence_records VALUES(?1,?2,?3,?4) ON CONFLICT(kind,id) DO UPDATE SET payload=excluded.payload", params![kind, id, at, encode(value)?])?;
    Ok(())
}

pub fn records<T: DeserializeOwned>(
    db: &Database,
    kind: &str,
    limit: usize,
) -> Result<Vec<T>, String> {
