use std::time::Duration;

use anyhow::{Context, Error, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use futures::StreamExt;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::types::Json;
use sqlx::{ConnectOptions, Pool, Postgres};
use tracing::log::LevelFilter;
use tracing::{event, Level};

use crate::api_client::api_model::{RegistrationResponse, Waypoint};
use crate::configuration::AgentConfiguration;
use crate::st_client::Data;
use crate::st_model::{
    MarketData, StStatusResponse, SystemSymbol, WaypointInSystemResponseData, WaypointSymbol,
    WaypointTraitSymbol,
};

pub async fn prepare_database_schema(
    api_status: StStatusResponse,
    cfg: AgentConfiguration,
) -> Result<Pool<Postgres>> {
    event!(Level::INFO, "Got status: {:?}", api_status);

    event!(Level::INFO, "Agent config: {:?}", cfg);

    event!(
        Level::INFO,
        "Postgres connection string: '{:?}'",
        cfg.pg_connection_string()
    );

    let database_url = cfg.database_url.clone();

    let database_connection_options: PgConnectOptions = database_url
        .parse::<PgConnectOptions>()?
        .log_slow_statements(LevelFilter::Warn, Duration::from_secs(60));

    let pg_connection_pool: Pool<Postgres> = PgPoolOptions::new()
        .max_connections(5)
        .connect_with(database_connection_options)
        .await?;

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
                    cfg.get_schema_name_for_reset_date(db_status.reset_date.clone());
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

async fn load_status(pool: &Pool<Postgres>) -> Result<Option<DbStatus>, Error> {
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

struct DbStatus {
    reset_date: String,
    entry: Json<StStatusResponse>,
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
    pub entry: Json<WaypointInSystemResponseData>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize, Clone, Debug, Deserialize)]
pub struct DbMarketEntry {
    pub waypoint_symbol: String,
    pub entry: Json<MarketData>,
    pub created_at: DateTime<Utc>,
}

pub async fn upsert_waypoints_from_receiver(
    pool: &Pool<Postgres>,
    mut rx: tokio::sync::mpsc::Receiver<(Vec<WaypointInSystemResponseData>, DateTime<Utc>)>,
) -> Result<()> {
    while let Some((entries, now)) = rx.recv().await {
        upsert_waypoints_of_system(pool, entries, now).await?;
    }
    Ok(())
}

pub async fn upsert_waypoints_of_system(
    pool: &Pool<Postgres>,
    waypoints: Vec<WaypointInSystemResponseData>,
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

    if let Some(first) = first_vec.get(0) {
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

        let json_array = serde_json::to_value(&rest)?;
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
            waypoint_symbol: me.symbol.clone(),
            entry: Json(me.clone()),

            created_at: now,
        })
        .collect();

    let (first_vec, rest) = db_entries.split_at(1);

    if let Some(first) = first_vec.get(0) {
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

        let json_array = serde_json::to_value(&rest)?;
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
    system_symbol: SystemSymbol,
) -> Result<Vec<DbWaypointEntry>> {
    let waypoint_entries: Vec<DbWaypointEntry> = sqlx::query_as!(
        DbWaypointEntry,
        r#"
select system_symbol
     , waypoint_symbol
     , entry as "entry: Json<WaypointInSystemResponseData>"
     , created_at
     , updated_at
from waypoints
where system_symbol = $1
    "#,
        system_symbol.0
    )
    .fetch_all(pool)
    .await?;

    Ok(waypoint_entries)
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
        trait_symbol.0,
        system_symbol.0
    )
    .fetch_all(pool)
    .await?;

    Ok(waypoint_symbols.into_iter().map(WaypointSymbol).collect())
}

pub async fn select_latest_marketplace_entry_of_system(
    pool: &Pool<Postgres>,
    system_symbol: SystemSymbol,
) -> Result<Vec<DbMarketEntry>> {
    let market_data_entries: Vec<DbMarketEntry> = sqlx::query_as!(
        DbMarketEntry,
        r#"
with latest_markets as (select DISTINCT ON (waypoint_symbol) waypoint_symbol, entry, created_at
                        from markets m
                        order by waypoint_symbol, entry, created_at desc)
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
