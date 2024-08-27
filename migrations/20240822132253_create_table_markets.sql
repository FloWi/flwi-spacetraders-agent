-- Add migration script here
create table markets
(
    waypoint_symbol text        not null,
    entry           jsonb       not null,
    created_at      timestamptz not null default now(),
    primary key (waypoint_symbol, created_at)
);
