use std::time::Duration;

use anyhow::{Error, Result};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::types::Json;
use sqlx::{ConnectOptions, Pool, Postgres};
use tracing::log::LevelFilter;
use tracing::{event, Level};

use st_domain::{
    distance_to, Data, GetConstructionResponse, JumpGate, MarketData, RegistrationResponse, Ship,
    Shipyard, StStatusResponse, SystemSymbol, SystemsPageData, Waypoint, WaypointSymbol,
    WaypointTraitSymbol,
};

#[derive(Clone)]
pub struct PgConnectionString(pub String);

impl PgConnectionString {
    pub fn get_schema_name_for_reset_date(&self, reset_date: String) -> String {
        format!("reset_{}", reset_date.replace("-", "_"))
    }
}

pub async fn get_pg_connection_pool(
    connection_string: PgConnectionString,
) -> Result<Pool<Postgres>> {
    let database_url = connection_string.0.clone();

    let database_connection_options: PgConnectOptions = database_url
        .parse::<PgConnectOptions>()?
        .log_slow_statements(LevelFilter::Warn, Duration::from_secs(60));

    let pg_connection_pool: Pool<Postgres> = PgPoolOptions::new()
        .max_connections(5)
        .connect_with(database_connection_options)
        .await?;

    Ok(pg_connection_pool)
}

pub async fn prepare_database_schema(
    api_status: &StStatusResponse,
    connection_string: PgConnectionString,
) -> Result<Pool<Postgres>> {
    event!(Level::INFO, "Got status: {:?}", api_status);

    event!(
        Level::INFO,
        "Postgres connection string: '{:?}'",
        connection_string.0
    );

    let pg_connection_pool = get_pg_connection_pool(connection_string.clone()).await?;

    perform_migration(&pg_connection_pool).await?;

    match load_status(&pg_connection_pool).await? {
        None => {
            event!(
                Level::INFO,
                "No entry for reset {} in db found.",
                api_status.reset_date
            );
            insert_status(
                &pg_connection_pool,
                DbStatus {
                    reset_date: api_status.reset_date.clone(),
                    entry: Json(api_status.clone()),
                },
            )
            .await?;

            Ok(pg_connection_pool)
        }
        Some(db_status) => {
            if db_status.reset_date == api_status.reset_date {
                event!(
                    Level::INFO,
                    "Current schema matches the reset-date {}.",
                    db_status.reset_date
                );
                Ok(pg_connection_pool)
            } else {
                let archive_schema_name =
                    connection_string.get_schema_name_for_reset_date(db_status.reset_date.clone());
                event!(
                    Level::INFO,
                    "Current schema public is for reset '{}', but the api is for reset '{}'. Archiving public schema to {}",
                    db_status.reset_date,
                    api_status.reset_date, archive_schema_name
                );
                rename_schema(&pg_connection_pool, "public", archive_schema_name).await?;
                create_schema(&pg_connection_pool, "public").await?;
                perform_migration(&pg_connection_pool).await?;

                Ok(pg_connection_pool)
            }
        }
    }
}

async fn perform_migration(pg_connection_pool: &Pool<Postgres>) -> Result<()> {
    event!(Level::INFO, "Migrating database if necessary");
    sqlx::migrate!().run(pg_connection_pool).await?;
    event!(Level::INFO, "Done migrating database");

    Ok(())
}

async fn rename_schema(
    pg_connection_pool: &Pool<Postgres>,
    from_schema_name: &str,
    to_schema_name: String,
) -> Result<()> {
    // Rename the current public schema
    sqlx::query(&format!(
        "ALTER SCHEMA {} RENAME TO {}",
        from_schema_name, to_schema_name
    ))
    .execute(pg_connection_pool)
    .await?;

    Ok(())
}

async fn create_schema(pg_connection_pool: &Pool<Postgres>, schema_name: &str) -> Result<()> {
    // Rename the current public schema
    sqlx::query(&format!("CREATE SCHEMA {}", schema_name))
        .execute(pg_connection_pool)
        .await?;

    Ok(())
}

pub(crate) async fn load_status(pool: &Pool<Postgres>) -> Result<Option<DbStatus>, Error> {
    let maybe_result = sqlx::query_as!(
        DbStatus,
        r#"
select reset_date
     , entry as "entry: Json<StStatusResponse>"
  from status
  limit 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    Ok(maybe_result)
}

async fn insert_status(pool: &Pool<Postgres>, db_status: DbStatus) -> Result<()> {
    sqlx::query!(
        r#"
insert into status (reset_date, entry)
values ($1, $2)
on conflict (reset_date) do nothing
        "#,
        db_status.reset_date,
        db_status.entry as _
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub struct DbStatus {
    reset_date: String,
    pub entry: Json<StStatusResponse>,
}

pub struct DbRegistrationResponse {
    pub token: String,
    pub entry: Json<Data<RegistrationResponse>>,
}

pub async fn load_registration(pool: &Pool<Postgres>) -> Result<Option<DbRegistrationResponse>> {
    let maybe_result = sqlx::query_as!(
        DbRegistrationResponse,
        r#"
select token
     , entry as "entry: Json<Data<RegistrationResponse>>"
  from registration
  limit 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    Ok(maybe_result)
}

pub async fn save_registration(
    pool: &Pool<Postgres>,
    api_registration_response: Data<RegistrationResponse>,
) -> Result<()> {
    sqlx::query!(
        r#"
insert into registration (token, entry)
values ($1, $2)
        "#,
        api_registration_response.data.token,
        Json(api_registration_response.clone()) as _
    )
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct DbWaypointEntry {
    system_symbol: String,
    waypoint_symbol: String,
    pub entry: Json<Waypoint>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct DbSystemEntry {
    system_symbol: String,
    pub entry: Json<SystemsPageData>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct DbMarketEntry {
    pub waypoint_symbol: String,
    pub entry: Json<MarketData>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct DbJumpGateData {
    pub system_symbol: String,
    pub waypoint_symbol: String,
    pub entry: Json<JumpGate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct DbShipyardData {
    pub system_symbol: String,
    pub waypoint_symbol: String,
    pub entry: Json<Shipyard>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DbShipyardData {
    pub fn has_detailed_price_information(&self) -> bool {
        !self.entry.0.ships.is_empty()
    }
}

#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct DbShipEntry {
    pub ship_symbol: String,
    pub entry: Json<Ship>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct DbConstructionSiteEntry {
    pub waypoint_symbol: String,
    pub entry: Json<GetConstructionResponse>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct DbSystemCoordinateData {
    pub system_symbol: String,
    pub x: i64,
    pub y: i64,
}

impl DbSystemCoordinateData {
    pub fn distance_to(&self, to: &&DbSystemCoordinateData) -> u32 {
        distance_to(self.x, self.y, to.x, to.y)
    }
}

pub async fn upsert_waypoints_from_receiver(
    pool: &Pool<Postgres>,
    mut rx: tokio::sync::mpsc::Receiver<(Vec<Waypoint>, DateTime<Utc>)>,
) -> Result<()> {
    while let Some((entries, now)) = rx.recv().await {
        upsert_waypoints(pool, entries, now).await?;
    }
    Ok(())
}

pub async fn upsert_systems_from_receiver(
    pool: &Pool<Postgres>,
    mut rx: tokio::sync::mpsc::Receiver<(Vec<SystemsPageData>, DateTime<Utc>)>,
) -> Result<()> {
    while let Some((entries, now)) = rx.recv().await {
        upsert_systems_page(pool, entries, now).await?;
    }
    Ok(())
}

pub async fn upsert_systems_page(
    pool: &Pool<Postgres>,
    systems: Vec<SystemsPageData>,
    now: DateTime<Utc>,
) -> Result<()> {
    let db_entries: Vec<DbSystemEntry> = systems
        .iter()
        .map(|system| DbSystemEntry {
            system_symbol: system.symbol.0.clone(),
            entry: Json(system.clone()),
            created_at: now,
            updated_at: now,
        })
        .collect();

    for entry in db_entries {
        sqlx::query!(
            r#"
insert into systems (system_symbol, entry, created_at, updated_at)
values ($1, $2, $3, $4)
on conflict (system_symbol) do UPDATE set entry = excluded.entry, updated_at = excluded.updated_at
        "#,
            entry.system_symbol,
            entry.entry as _,
            now,
            now,
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn upsert_waypoints(
    pool: &Pool<Postgres>,
    waypoints: Vec<Waypoint>,
    now: DateTime<Utc>,
) -> Result<()> {
    let db_entries: Vec<DbWaypointEntry> = waypoints
        .iter()
        .map(|wp| DbWaypointEntry {
            system_symbol: wp.system_symbol.0.clone(),
            waypoint_symbol: wp.symbol.0.clone(),
            entry: Json(wp.clone()),
            created_at: now,
            updated_at: now,
        })
        .collect();

    let (first_vec, rest) = db_entries.split_at(1);

    if let Some(first) = first_vec.first() {
        // insert first entry manually to get sqlx compile-time check

        sqlx::query!(
            r#"
insert into waypoints (system_symbol, waypoint_symbol, entry, created_at, updated_at)
values ($1, $2, $3, $4, $5)
on conflict (waypoint_symbol) do UPDATE set entry = excluded.entry, updated_at = excluded.updated_at
        "#,
            first.system_symbol,
            first.waypoint_symbol,
            first.entry as _,
            now,
            now,
        )
        .execute(pool)
        .await?;

        let json_array = serde_json::to_value(rest)?;
        let debug_string = json_array.to_string();

        sqlx::query!(
            r#"
insert into waypoints
select *
from jsonb_populate_recordset(NULL::waypoints, $1)
on conflict (waypoint_symbol) do UPDATE set entry = excluded.entry, updated_at = excluded.updated_at
            "#,
            json_array
        )
        .execute(pool)
        .await?;

        Ok(())
    } else {
        Ok(())
    }
}

pub async fn insert_market_data(
    pool: &Pool<Postgres>,
    market_entries: Vec<MarketData>,
    now: DateTime<Utc>,
) -> Result<()> {
    let db_entries: Vec<DbMarketEntry> = market_entries
        .iter()
        .map(|me| DbMarketEntry {
            waypoint_symbol: me.symbol.0.clone(),
            entry: Json(me.clone()),

            created_at: now,
        })
        .collect();

    let (first_vec, rest) = db_entries.split_at(1);

    if let Some(first) = first_vec.first() {
        // insert first entry manually to get sqlx compile-time check
        sqlx::query!(
            r#"
insert into markets (waypoint_symbol, entry, created_at)
values ($1, $2, $3)
        "#,
            first.waypoint_symbol,
            first.entry as _,
            now,
        )
        .execute(pool)
        .await?;

        let json_array = serde_json::to_value(rest)?;
        let debug_string = json_array.clone().to_string();

        sqlx::query!(
            r#"
insert into markets
select *
from jsonb_populate_recordset(NULL::markets, $1)
            "#,
            json_array
        )
        .execute(pool)
        .await?;

        Ok(())
    } else {
        Ok(())
    }
}

pub async fn select_waypoints_of_system(
    pool: &Pool<Postgres>,
    system_symbol: &SystemSymbol,
) -> Result<Vec<Waypoint>> {
    let waypoint_entries: Vec<DbWaypointEntry> = sqlx::query_as!(
        DbWaypointEntry,
        r#"
select system_symbol
     , waypoint_symbol
     , entry as "entry: Json<Waypoint>"
     , created_at
     , updated_at
from waypoints
where system_symbol = $1
    "#,
        system_symbol.0
    )
    .fetch_all(pool)
    .await?;

    Ok(waypoint_entries.iter().map(|db_waypoint_entry| db_waypoint_entry.entry.0.clone()).collect_vec())
}

pub async fn select_ships(
    pool: &Pool<Postgres>,
) -> Result<Vec<Ship>> {
    let ship_entries: Vec<DbShipEntry> = sqlx::query_as!(
        DbShipEntry,
        r#"
select ship_symbol
     , entry as "entry: Json<Ship>"
     , created_at
     , updated_at
from ships
    "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(ship_entries.iter().map(|db_ship| db_ship.entry.0.clone()).collect_vec())
}

pub async fn select_waypoints_of_system_with_trait(
    pool: &Pool<Postgres>,
    system_symbol: SystemSymbol,
    trait_symbol: WaypointTraitSymbol,
) -> Result<Vec<WaypointSymbol>> {
    // lots of typecasting necessary to convince sqlx that $1 _is_ a text parameter :-/
    let waypoint_symbols: Vec<String> = sqlx::query_scalar!(
        r#"
        SELECT waypoint_symbol
        FROM waypoints
        WHERE jsonb_path_exists(entry, ('$.traits[*] ? (@.symbol == "' || $1::text || '")')::jsonpath)
        AND system_symbol = $2
    "#,
        trait_symbol.to_string(),
        system_symbol.0
    )
    .fetch_all(pool)
    .await?;

    Ok(waypoint_symbols.into_iter().map(WaypointSymbol).collect())
}

pub async fn select_latest_marketplace_entry_of_system(
    pool: &Pool<Postgres>,
    system_symbol: &SystemSymbol,
) -> Result<Vec<DbMarketEntry>> {
    let market_data_entries: Vec<DbMarketEntry> = sqlx::query_as!(
        DbMarketEntry,
        r#"
with latest_markets as (select DISTINCT ON (waypoint_symbol) waypoint_symbol, entry, created_at
                        from markets m
                        order by waypoint_symbol, created_at desc, entry)
   , market_entries as (select w.system_symbol
                             , m.waypoint_symbol
                             , m.entry
                             , m.created_at
                        from latest_markets m
                                 join waypoints w on m.waypoint_symbol = w.waypoint_symbol)
select waypoint_symbol
     , entry as "entry: Json<MarketData>"
     , created_at
from market_entries
where system_symbol = $1
    "#,
        system_symbol.0
    )
    .fetch_all(pool)
    .await?;

    Ok(market_data_entries)
}
pub async fn select_latest_shipyard_entry_of_system(
    pool: &Pool<Postgres>,
    system_symbol: &SystemSymbol,
) -> Result<Vec<DbShipyardData>> {
    let db_entries: Vec<DbShipyardData> = sqlx::query_as!(
        DbShipyardData,
        r#"
select system_symbol
     , waypoint_symbol
     , entry as "entry: Json<Shipyard>"
     , created_at
     , updated_at
from shipyards
where system_symbol = $1
    "#,
        system_symbol.0
    )
    .fetch_all(pool)
    .await?;

    Ok(db_entries)
}

pub async fn select_systems_with_waypoint_details_to_be_loaded(
    pool: &Pool<Postgres>,
) -> Result<Vec<DbSystemCoordinateData>> {
    let entries: Vec<DbSystemCoordinateData> = sqlx::query_as!(
        DbSystemCoordinateData,
        r#"
with details as (select s.system_symbol
                  , (s.entry ->> 'x') :: int                   as x
                  , (s.entry ->> 'y') :: int                   as y
                  , count(w.*)                                 as num_entries_in_waypoint_table
                  , jsonb_array_length(s.entry -> 'waypoints') as num_waypoints_in_system_json
             from systems s
                      left join waypoints w using (system_symbol)
             group by s.system_symbol, s.entry)
select system_symbol
     , x as "x!: i64"
     , y as "y!: i64"
from details
where num_waypoints_in_system_json > 0
  and num_waypoints_in_system_json != num_entries_in_waypoint_table
"#,
    )
    .fetch_all(pool)
    .await?;

    Ok(entries)
}

pub async fn select_system_with_coordinate(
    pool: &Pool<Postgres>,
    system_symbol: &SystemSymbol,
) -> Result<Option<DbSystemCoordinateData>> {
    let maybe_system: Option<DbSystemCoordinateData> = sqlx::query_as!(
        DbSystemCoordinateData,
        r#"
select system_symbol
     , (s.entry ->> 'x') :: int as "x!: i64"
     , (s.entry ->> 'y') :: int as "y!: i64"
from systems s
where system_symbol = $1
"#,
        system_symbol.0
    )
    .fetch_optional(pool)
    .await?;

    Ok(maybe_system)
}

pub async fn select_system(
    pool: &Pool<Postgres>,
    system_symbol: &SystemSymbol,
) -> Result<Option<SystemsPageData>> {
    /*
    #[derive(Serialize, Clone, Debug, Deserialize)]
    pub struct DbSystemEntry {
        system_symbol: String,
        pub entry: Json<SystemsPageData>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    }
         */
    let maybe_system: Option<DbSystemEntry> = sqlx::query_as!(
        DbSystemEntry,
        r#"
select system_symbol
     , entry as "entry: Json<SystemsPageData>"
     , created_at
     , updated_at
    from systems s
where system_symbol = $1
"#,
        system_symbol.0
    )
    .fetch_optional(pool)
    .await?;

    Ok(maybe_system.map(|db_system| db_system.entry.0))
}

pub(crate) async fn select_jump_gate(
    pool: &Pool<Postgres>,
    waypoint_symbol: &WaypointSymbol,
) -> Result<Option<DbJumpGateData>> {
    let maybe_jump_gate_data: Option<DbJumpGateData> = sqlx::query_as!(
        DbJumpGateData,
        r#"
select system_symbol
     , waypoint_symbol
     , entry as "entry: Json<JumpGate>"
     , created_at
     , updated_at
from waypoints
where system_symbol = $1
    "#,
        waypoint_symbol.0
    )
    .fetch_optional(pool)
    .await?;

    Ok(maybe_jump_gate_data)
}

pub async fn insert_jump_gates(
    pool: &Pool<Postgres>,
    jump_gates: Vec<JumpGate>,
    now: DateTime<Utc>,
) -> Result<()> {
    let db_entries: Vec<DbJumpGateData> = jump_gates
        .iter()
        .map(|j| DbJumpGateData {
            system_symbol: j.symbol.clone().system_symbol().0,
            waypoint_symbol: j.symbol.clone().0,
            entry: Json(j.clone()),
            created_at: now,
            updated_at: now,
        })
        .collect();

    for entry in db_entries {
        sqlx::query!(
            r#"
insert into jump_gates (system_symbol, waypoint_symbol, entry, created_at, updated_at)
values ($1, $2, $3, $4, $5)
on conflict (system_symbol, waypoint_symbol) do UPDATE set entry = excluded.entry, updated_at = excluded.updated_at
        "#,
            entry.system_symbol,
            entry.waypoint_symbol,
            entry.entry as _,
            now,
            now,
        )
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn insert_shipyards(
    pool: &Pool<Postgres>,
    shipyards: Vec<Shipyard>,
    now: DateTime<Utc>,
) -> Result<()> {
    let db_entries: Vec<DbShipyardData> = shipyards
        .iter()
        .map(|j| DbShipyardData {
            system_symbol: j.symbol.clone().system_symbol().0,
            waypoint_symbol: j.symbol.clone().0,
            entry: Json(j.clone()),
            created_at: now,
            updated_at: now,
        })
        .collect();

    for entry in db_entries {
        sqlx::query!(
            r#"
insert into shipyards (system_symbol, waypoint_symbol, entry, created_at, updated_at)
values ($1, $2, $3, $4, $5)
on conflict (system_symbol, waypoint_symbol) do UPDATE set entry = excluded.entry, updated_at = excluded.updated_at
        "#,
            entry.system_symbol,
            entry.waypoint_symbol,
            entry.entry as _,
            now,
            now,
        )
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn select_count_of_systems(pool: &Pool<Postgres>) -> Result<i64> {
    let count: Option<i64> = sqlx::query_scalar!(
        r#"
select count(*)
from systems s
"#,
    )
    .fetch_one(pool)
    .await?;

    Ok(count.unwrap_or(0))
}

pub async fn upsert_ships(
    pool: &Pool<Postgres>,
    ships: &Vec<Ship>,
    now: DateTime<Utc>,
) -> Result<()> {
    let db_entries: Vec<DbShipEntry> = ships
        .iter()
        .map(|ship| DbShipEntry {
            ship_symbol: ship.symbol.0.clone(),
            entry: Json(ship.clone()),
            created_at: now,
            updated_at: now,
        })
        .collect();

    let (first_vec, rest) = db_entries.split_at(1);

    if let Some(first) = first_vec.first() {
        // insert first entry manually to get sqlx compile-time check

        sqlx::query!(
            r#"
insert into ships (ship_symbol, entry, created_at, updated_at)
values ($1, $2, $3, $4)
on conflict (ship_symbol) do UPDATE set entry = excluded.entry, updated_at = excluded.updated_at
        "#,
            first.ship_symbol,
            first.entry as _,
            now,
            now,
        )
        .execute(pool)
        .await?;

        let json_array = serde_json::to_value(rest)?;
        let debug_string = json_array.to_string();

        sqlx::query!(
            r#"
insert into ships
select *
from jsonb_populate_recordset(NULL::ships, $1)
on conflict (ship_symbol) do UPDATE set entry = excluded.entry, updated_at = excluded.updated_at
            "#,
            json_array
        )
        .execute(pool)
        .await?;

        Ok(())
    } else {
        Ok(())
    }
}

pub async fn upsert_construction_site(
    pool: &Pool<Postgres>,
    construction: GetConstructionResponse,
    now: DateTime<Utc>,
) -> Result<()> {
    let db_entry: DbConstructionSiteEntry = DbConstructionSiteEntry {
        waypoint_symbol: construction.data.symbol.clone(),
        entry: Json(construction.clone()),
        created_at: now,
        updated_at: now,
    };

    sqlx::query!(
        r#"
insert into construction_sites (waypoint_symbol, entry, created_at, updated_at)
values ($1, $2, $3, $4)
on conflict (waypoint_symbol) do UPDATE set entry = excluded.entry, updated_at = excluded.updated_at
        "#,
        db_entry.waypoint_symbol,
        db_entry.entry as _,
        now,
        now,
    )
    .execute(pool)
    .await?;

    Ok(())
}
