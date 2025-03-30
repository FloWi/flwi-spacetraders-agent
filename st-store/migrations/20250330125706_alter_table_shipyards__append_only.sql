-- Add migration script here

ALTER TABLE shipyards
    DROP CONSTRAINT shipyards_pkey;

ALTER TABLE shipyards
    DROP COLUMN updated_at;

-- Add the new primary key constraint including created_at
ALTER TABLE shipyards
    ADD PRIMARY KEY (system_symbol, waypoint_symbol, created_at);
