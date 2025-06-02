-- Add migration script here
create table survey_usage_log
(
    survey_signature text        not null,
    extraction       jsonb       not null,
    created_at       timestamptz not null
);
