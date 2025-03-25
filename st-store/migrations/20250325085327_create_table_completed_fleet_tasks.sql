create table completed_fleet_tasks
(
    task         jsonb       not null,
    completed_at timestamptz not null default now()
);
