-- Add migration script here
create table ledger_entries
(
    entry      jsonb       not null,
    created_at timestamptz not null

);

create index ix_ledger_entries_created_at on ledger_entries (created_at);
