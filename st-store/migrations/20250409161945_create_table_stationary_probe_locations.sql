-- Add migration script here
create table stationary_probe_locations
(
    waypoint_symbol   text  not null primary key,
    probe_ship_symbol text  not null,
    exploration_tasks jsonb not null

)
