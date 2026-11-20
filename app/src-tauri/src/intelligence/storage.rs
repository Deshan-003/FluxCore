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
            tx.execute("DELETE FROM intelligence_records WHERE recorded_at<?1", [row.at - 30 * 86400])?;
        }
        tx.commit()
    })
}

pub fn decide(
    db: &Database,
    rec: Option<&Recommendation>,
    action: Option<&Action>,
) -> Result<(), String> {
    db.with_connection(|conn| {
        let tx = conn.unchecked_transaction()?;
        if let Some(rec) = rec {
            record(&tx, "recommendation", &rec.id, rec.created_at, rec)?;
        }
        if let Some(action) = action {
            record(&tx, "action", &action.id, action.at, action)?;
        }
        tx.commit()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_preserves_existing_gpu_config_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flux.sqlite");
        let db = Database::open_at_path(path.clone()).unwrap();
        let gpu = crate::gpu_engine::models::AccelerationConfig {
            max_concurrent_gpu_tasks: 2,
            ..Default::default()
        };
        crate::gpu_engine::persistence::save_config(&db, &gpu).unwrap();
        db.with_connection(|c| c.execute_batch("DROP TABLE intelligence_observations; DROP TABLE intelligence_forecasts; DROP TABLE intelligence_records; DROP TABLE intelligence_policy; DELETE FROM schema_migrations WHERE version=5;")).unwrap();
        drop(db);
        let db = Database::open_at_path(path.clone()).unwrap();
        assert_eq!(
            crate::gpu_engine::persistence::load_config(&db)
                .unwrap()
                .unwrap()
                .max_concurrent_gpu_tasks,
            2
        );
        let policy = Policy {
            mode: Mode::SmartAssist,
            ..Policy::default()
        };
        configure(&db, &policy, None).unwrap();
        drop(db);
        assert_eq!(
            super::policy(&Database::open_at_path(path).unwrap())
                .unwrap()
                .mode,
            Mode::SmartAssist
        );
    }

    #[test]
    fn forecast_comparison_uses_only_real_observations_near_target() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_at_path(dir.path().join("flux.sqlite")).unwrap();
        let row = |at| Observation {
            at,
            values: [("cpu".into(), 18.0)].into(),
            ..Default::default()
        };
        let mut f = super::super::statistics::forecasts(&[row(180)], "cpu", &[1, 2]);
        for forecast in &mut f {
            forecast.value = Some(11.0);
        }
        sample(&db, &row(180), &f, &[], &[], false).unwrap();
        sample(&db, &row(180), &f, &[], &[], false).unwrap();
        sample(&db, &row(252), &[], &[], &[], false).unwrap();
        sample(&db, &row(350), &[], &[], &[], false).unwrap();
        assert_eq!(observations(&db, 0, 20).unwrap().len(), 3);
        assert_eq!(comparisons(&db, "cpu", 1).unwrap()[0].actual, Some(18.0));
        assert!(comparisons(&db, "cpu", 2).unwrap().is_empty());
    }
}
