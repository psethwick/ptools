-- Change duration_seconds column to duration and change type from INTEGER to TEXT
-- This migration converts from seconds to Jira-style decimal hours format

-- Add new column as TEXT
ALTER TABLE timesheet ADD COLUMN duration TEXT;

-- Convert existing data from seconds to decimal hours format
UPDATE timesheet 
SET duration = ROUND(duration_seconds / 3600.0, 2) || 'h';

-- Remove the old column
ALTER TABLE timesheet DROP COLUMN duration_seconds;