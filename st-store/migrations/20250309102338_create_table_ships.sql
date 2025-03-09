create table ships
(
    ship_symbol text        not null primary key,
    entry       jsonb       not null,
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now()
);
