CREATE TABLE remote (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind INTEGER NOT NULL,
  name TEXT NOT NULL,
  UNIQUE (kind, name)
);

CREATE TABLE work (
  remote_id INTEGER NOT NULL,
  id TEXT NOT NULL,
  project TEXT NOT NULL,
  title TEXT NOT NULL,
  parent_id TEXT,
  description TEXT,
  work_type TEXT NOT NULL,
  version TEXT,
  state TEXT,
  created_by_id TEXT,
  assigned_to_id TEXT,
  column TEXT,
  created TIMESTAMP,
  modified TIMESTAMP,
  url TEXT,
  PRIMARY KEY (remote_id, id),
  FOREIGN KEY (remote_id) REFERENCES remote (id)
);

CREATE TABLE person (
  remote_id INTEGER NOT NULL,
  id TEXT NOT NULL,
  name TEXT NOT NULL,
  PRIMARY KEY (remote_id, id),
  FOREIGN KEY (remote_id) REFERENCES remote (id)
);
