-- Add migration script here
create table surveys
(
    waypoint_symbol text        not null,
    signature       text        not null,
    entry           jsonb       not null,
    created_at      timestamptz not null,
    expires_at      timestamptz not null,
    is_discarded    boolean     not null,
    primary key (waypoint_symbol, signature)
);

create index ix_surveys_waypoint_symbol_expires_at on surveys (waypoint_symbol, is_discarded, expires_at desc);
