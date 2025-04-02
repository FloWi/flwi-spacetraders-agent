-- Add migration script here

create table agent
(
    agent_symbol text  not null PRIMARY KEY,
    entry        jsonb not null
);
