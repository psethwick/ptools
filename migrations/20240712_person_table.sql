CREATE TABLE IF NOT EXISTS person (
  source TEXT NOT null,
  id TEXT NOT NULL,
  name TEXT NOT NULL,
  PRIMARY KEY (source, id)
);
