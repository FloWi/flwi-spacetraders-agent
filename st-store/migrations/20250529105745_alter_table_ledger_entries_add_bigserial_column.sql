-- Add migration script here
alter table ledger_entries
    add column if not exists id bigserial primary key;

-- Update the sequence to assign values based on created_at order
WITH ordered_rows AS (SELECT ctid, ROW_NUMBER() OVER (ORDER BY created_at) as new_id
                      FROM ledger_entries)
UPDATE ledger_entries
SET id = ordered_rows.new_id
FROM ordered_rows
WHERE ledger_entries.ctid = ordered_rows.ctid;

