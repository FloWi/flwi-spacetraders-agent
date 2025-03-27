-- Add migration script here
create table fleets
(
    id  serial not null primary key,
    cfg jsonb  not null
);

create table fleet_task_assignments
(
    fleet_id int   not null primary key references fleets (id),
    tasks    jsonb not null
);

create table fleet_ship_assignment
(
    ship_symbol text not null references ships (ship_symbol) primary key,
    fleet_id    int  not null references fleets (id)
);

create table ship_task_assignments
(
    ship_symbol text  not null primary key references ships (ship_symbol),
    task        jsonb not null
);
