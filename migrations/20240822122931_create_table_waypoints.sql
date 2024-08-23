-- Add migration script here
create table waypoints
(
    system_symbol   text        not null,
    waypoint_symbol text        not null,
    entry           jsonb       not null,
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now(),
    primary key (system_symbol, waypoint_symbol)
);
create unique index ux_waypoints__waypoint_symbol on waypoints (waypoint_symbol);
