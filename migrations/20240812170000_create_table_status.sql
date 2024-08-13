create table status
(
    reset_date text not null primary key,
    entry      json not null
);
