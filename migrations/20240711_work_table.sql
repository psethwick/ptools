CREATE TABLE IF NOT EXISTS work (
  source TEXT NOT null,
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
  PRIMARY KEY (source, id)
);
