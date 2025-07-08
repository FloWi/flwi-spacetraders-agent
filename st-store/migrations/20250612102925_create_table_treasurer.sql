-- Add migration script here
create table treasurer
(
    from_ledger_id bigint      not null primary key,
    to_ledger_id   bigint      not null,
    entry          jsonb       not null,
    created_at     timestamptz not null
)
