-- Update the person table to use a foreign key to the source table.
PRAGMA foreign_keys=off;

CREATE TABLE person_new (
  source_id INTEGER NOT NULL,
  id TEXT NOT NULL,
  name TEXT NOT NULL,
  PRIMARY KEY (source_id, id),
  FOREIGN KEY (source_id) REFERENCES source(id)
);

INSERT INTO person_new (source_id, id, name)
SELECT
  s.id,
  p.id,
  p.name
FROM person p
JOIN source s ON SUBSTR(p.source, 1, INSTR(p.source, '-') - 1) = s.kind AND SUBSTR(p.source, INSTR(p.source, '-') + 1) = s.name;

DROP TABLE person;

ALTER TABLE person_new RENAME TO person;

PRAGMA foreign_keys=on;
