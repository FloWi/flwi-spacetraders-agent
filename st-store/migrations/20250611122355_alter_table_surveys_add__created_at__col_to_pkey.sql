-- Step 1: Drop the existing primary key constraint
ALTER TABLE surveys
    DROP CONSTRAINT surveys_pkey;

-- Step 2: Create new primary key with created_at included
ALTER TABLE surveys
    ADD CONSTRAINT surveys_pkey
        PRIMARY KEY (waypoint_symbol, signature, created_at);
