create table trade_tickets
(
    ticket_id    uuid        not null primary key,
    ship_symbol  text        not null references ships (ship_symbol),
    entry        jsonb       not null,
    created_at   timestamptz not null,
    updated_at   timestamptz not null,
    completed_at timestamptz null
);
