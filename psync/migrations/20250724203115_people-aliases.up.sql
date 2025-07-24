CREATE TABLE alias (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  person_id TEXT NOT NULL,
  aka_id TEXT NOT NULL,
  FOREIGN KEY (person_id) REFERENCES person (id),
  FOREIGN KEY (aka_id) REFERENCES person (id)
);
