-- Revert duration from TEXT back to INTEGER
-- Convert back from decimal hours to seconds

-- Add back the old column as INTEGER
ALTER TABLE timesheet ADD COLUMN duration_seconds INTEGER;

-- Convert from decimal hours format back to seconds
UPDATE timesheet 
SET duration_seconds = CAST(
    CAST(REPLACE(REPLACE(duration, 'h', ''), 'H', '') AS REAL) * 3600 AS INTEGER
);

-- Remove the new column
ALTER TABLE timesheet DROP COLUMN duration;