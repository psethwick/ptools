CREATE TABLE source (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  secrets TEXT NOT NULL DEFAULT '[]',
  UNIQUE (kind, name)
);

CREATE TABLE work (
  source_id INTEGER NOT NULL,
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
  PRIMARY KEY (source_id, id),
  FOREIGN KEY (source_id) REFERENCES source (id)
);

CREATE TABLE person (
  source_id INTEGER NOT NULL,
  id TEXT NOT NULL,
  name TEXT NOT NULL,
  PRIMARY KEY (source_id, id),
  FOREIGN KEY (source_id) REFERENCES source(id)
);
