-- Add an auto-incrementing primary key to the source table.
PRAGMA foreign_keys=off;

CREATE TABLE source_new (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  UNIQUE (kind, name)
);

INSERT INTO source_new (kind, name) SELECT kind, name FROM source;

DROP TABLE source;

ALTER TABLE source_new RENAME TO source;

PRAGMA foreign_keys=on;
