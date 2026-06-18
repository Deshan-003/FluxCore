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
