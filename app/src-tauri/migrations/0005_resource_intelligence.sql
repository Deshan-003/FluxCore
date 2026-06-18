CREATE TABLE IF NOT EXISTS intelligence_observations (
    observed_at INTEGER PRIMARY KEY,
    payload TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS intelligence_forecasts (
    observed_at INTEGER NOT NULL,
    metric TEXT NOT NULL,
    horizon INTEGER NOT NULL,
    target_at INTEGER NOT NULL,
    actual REAL,
    checked INTEGER NOT NULL DEFAULT 0,
    payload TEXT NOT NULL,
    PRIMARY KEY (observed_at, metric, horizon)
);
CREATE INDEX IF NOT EXISTS idx_intelligence_forecasts_due ON intelligence_forecasts(checked, target_at);
CREATE TABLE IF NOT EXISTS intelligence_records (
    kind TEXT NOT NULL CHECK(kind IN ('anomaly', 'recommendation', 'action')),
    id TEXT NOT NULL,
    recorded_at INTEGER NOT NULL,
