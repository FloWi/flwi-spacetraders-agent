-- Add migration script here
create table contracts
(
    id            text        not null primary key,
    system_symbol text        not null,
    entry         jsonb       not null,
    created_at    timestamptz not null,
    updated_at    timestamptz not null
);

create index ix_contracts on contracts (id, created_at desc);
