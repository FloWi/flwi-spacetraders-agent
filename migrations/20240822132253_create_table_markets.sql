-- Add migration script here
create table markets
(
    waypoint_symbol text        not null,
    entry           jsonb       not null,
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now(),
    primary key (waypoint_symbol)
);
create unique index ux_market__waypoint_symbol on markets (waypoint_symbol);
