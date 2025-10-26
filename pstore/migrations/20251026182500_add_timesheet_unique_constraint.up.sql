-- Remove duplicates, keeping the one with the lowest id
DELETE FROM timesheet
WHERE id NOT IN (
    SELECT MIN(id)
    FROM timesheet
    GROUP BY remote_id, ticket_id, date
);

-- Add a unique constraint to prevent future duplicates
CREATE UNIQUE INDEX timesheet_unique_entry ON timesheet(remote_id, ticket_id, date);
