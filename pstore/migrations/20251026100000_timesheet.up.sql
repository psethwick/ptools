CREATE TABLE timesheet (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  source_id INTEGER NOT NULL,
  ticket_id TEXT NOT NULL,
  date TEXT NOT NULL,
  duration_seconds INTEGER NOT NULL,
  synced INTEGER NOT NULL DEFAULT 0,
  FOREIGN KEY (source_id) REFERENCES source (id)
);
