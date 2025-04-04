-- Add migration script here
create table transactions
(
    ticket_id             uuid        not null references trade_tickets (ticket_id),
    transaction_ticket_id uuid        not null,
    total_price           bigint      not null,
    ship_symbol           text        not null references ships (ship_symbol),
    tx_summary            jsonb       not null,
    completed_at          timestamptz not null,
    is_complete           bool        not null
)
