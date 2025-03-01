-- Add migration script here
create table jump_gates
(
    system_symbol   text        not null,
    waypoint_symbol text        not null,
    entry           jsonb       not null,
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now(),
    primary key (system_symbol, waypoint_symbol)
);
