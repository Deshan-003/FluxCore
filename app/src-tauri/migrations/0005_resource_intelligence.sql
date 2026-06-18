CREATE TABLE IF NOT EXISTS intelligence_observations (
    observed_at INTEGER PRIMARY KEY,
    payload TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS intelligence_forecasts (
    observed_at INTEGER NOT NULL,
    metric TEXT NOT NULL,
    horizon INTEGER NOT NULL,
    target_at INTEGER NOT NULL,
