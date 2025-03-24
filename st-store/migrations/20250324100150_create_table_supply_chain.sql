create table supply_chain
(
    entry      jsonb       not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);
