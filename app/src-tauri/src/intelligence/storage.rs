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
    db.with_connection(|conn| {
        let mut stmt = conn.prepare_cached("SELECT payload FROM intelligence_records WHERE kind=?1 ORDER BY recorded_at DESC, id DESC LIMIT ?2")?;
        let rows = stmt.query_map(params![kind, limit as i64], |r| decode(r.get(0)?))?;
        rows.collect()
    })
}

pub fn observations(db: &Database, cutoff: i64, limit: usize) -> Result<Vec<Observation>, String> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare_cached("SELECT payload FROM intelligence_observations WHERE observed_at>=?1 ORDER BY observed_at DESC LIMIT ?2")?;
        let mut rows = stmt.query_map(params![cutoff, limit as i64], |r| decode(r.get(0)?))?.collect::<Result<Vec<Observation>, _>>()?;
        rows.reverse();
        Ok(rows)
    })
}

pub fn comparisons(db: &Database, metric: &str, horizon: i64) -> Result<Vec<Forecast>, String> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare_cached("SELECT payload, actual FROM intelligence_forecasts WHERE metric=?1 AND horizon=?2 AND checked=1 AND actual IS NOT NULL ORDER BY observed_at DESC LIMIT 120")?;
        let mut rows = stmt.query_map(params![metric,horizon], |r| {
            let mut forecast: Forecast = decode(r.get(0)?)?;
            forecast.actual = r.get(1)?;
            Ok(forecast)
        })?.collect::<Result<Vec<_>, _>>()?;
        rows.reverse();
        Ok(rows)
    })
}

pub fn sample(
    db: &Database,
    row: &Observation,
    forecasts: &[Forecast],
    anomalies: &[Anomaly],
    recommendations: &[Recommendation],
    cleanup: bool,
) -> Result<(), String> {
    db.with_connection(|conn| {
        let tx = conn.unchecked_transaction()?;
        tx.execute("INSERT OR IGNORE INTO intelligence_observations VALUES(?1,?2)", params![row.at, encode(row)?])?;
        for (metric, value) in &row.values {
            if super::analysis::forecast_metric(metric) {
                tx.execute("UPDATE intelligence_forecasts SET checked=1, actual=?1 WHERE checked=0 AND metric=?2 AND target_at BETWEEN ?3 AND ?4", params![value, metric, row.at - 30, row.at])?;
            }
        }
        tx.execute("UPDATE intelligence_forecasts SET checked=1 WHERE checked=0 AND target_at<?1", [row.at - 30])?;
        for forecast in forecasts.iter().filter(|f| f.value.is_some()) {
            tx.execute("INSERT OR IGNORE INTO intelligence_forecasts(observed_at,metric,horizon,target_at,payload) VALUES(?1,?2,?3,?4,?5)",
                params![forecast.observed_at, forecast.metric, forecast.horizon_minutes, forecast.target_at, encode(forecast)?])?;
        }
        for anomaly in anomalies { record(&tx, "anomaly", &anomaly.id, anomaly.started_at, anomaly)?; }
        for recommendation in recommendations { record(&tx, "recommendation", &recommendation.id, recommendation.created_at, recommendation)?; }
        if cleanup {
            tx.execute("DELETE FROM intelligence_observations WHERE observed_at<?1", [row.at - 7 * 86400])?;
            tx.execute("DELETE FROM intelligence_forecasts WHERE observed_at<?1", [row.at - 2 * 86400])?;
