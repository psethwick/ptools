-- Revert "Xh Ym" format back to "X.XXh" decimal format
-- This is lossy for sub-minute precision but matches the old format
UPDATE timesheet
SET duration = ROUND(
  (CASE
    WHEN duration LIKE '%h %m'
      THEN CAST(SUBSTR(duration, 1, INSTR(duration, 'h') - 1) AS REAL) +
           CAST(SUBSTR(duration, INSTR(duration, ' ') + 1, INSTR(duration, 'm') - INSTR(duration, ' ') - 1) AS REAL) / 60.0
    WHEN duration LIKE '%h'
      THEN CAST(REPLACE(duration, 'h', '') AS REAL)
    WHEN duration LIKE '%m'
      THEN CAST(REPLACE(duration, 'm', '') AS REAL) / 60.0
    ELSE 0.0
  END), 2) || 'h';
