CREATE TABLE timesheet (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  remote_id INTEGER NOT NULL,
  ticket_id TEXT NOT NULL,
  date TEXT NOT NULL,
  duration_seconds INTEGER NOT NULL,
  synced INTEGER NOT NULL DEFAULT 0,
  FOREIGN KEY (remote_id) REFERENCES remote (id)
);
