-- Update the work table to use a foreign key to the source table.
PRAGMA foreign_keys = off;

CREATE TABLE work_new (
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

INSERT INTO
  work_new (
    source_id,
    id,
    project,
    title,
    parent_id,
    description,
    work_type,
    version,
    state,
    created_by_id,
    assigned_to_id,
    column,
    created,
    modified,
    url
  )
SELECT
  s.id,
  w.id,
  w.project,
  w.title,
  w.parent_id,
  w.description,
  w.work_type,
  w.version,
  w.state,
  w.created_by_id,
  w.assigned_to_id,
  w.column,
  w.created,
  w.modified,
  w.url
FROM
  work w
  JOIN source s ON SUBSTR (w.source, 1, INSTR (w.source, '-') - 1) = s.kind
  AND SUBSTR (w.source, INSTR (w.source, '-') + 1) = s.name;

DROP TABLE work;

ALTER TABLE work_new
RENAME TO work;

PRAGMA foreign_keys = on;
