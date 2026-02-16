-- Convert duration from "X.XXh" decimal format to "Xh Ym" Jira-native format
UPDATE timesheet
SET duration =
  CASE
    WHEN CAST(CAST(REPLACE(duration, 'h', '') AS REAL) * 60 AS INTEGER) % 60 = 0
      THEN CAST(CAST(REPLACE(duration, 'h', '') AS REAL) * 60 / 60 AS INTEGER) || 'h'
    WHEN CAST(CAST(REPLACE(duration, 'h', '') AS REAL) * 60 / 60 AS INTEGER) = 0
      THEN (CAST(CAST(REPLACE(duration, 'h', '') AS REAL) * 60 AS INTEGER) % 60) || 'm'
    ELSE
      CAST(CAST(REPLACE(duration, 'h', '') AS REAL) * 60 / 60 AS INTEGER) || 'h ' ||
      (CAST(CAST(REPLACE(duration, 'h', '') AS REAL) * 60 AS INTEGER) % 60) || 'm'
  END
WHERE duration LIKE '%.%h';
