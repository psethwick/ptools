CREATE TABLE release (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    remote_id INTEGER NOT NULL,
    project TEXT NOT NULL,
    release_id TEXT NOT NULL,
    name TEXT NOT NULL,
    environment TEXT,
    started_at TEXT,
    deployed_at TEXT,
    status TEXT,
    url TEXT,
    UNIQUE(remote_id, project, release_id)
);
