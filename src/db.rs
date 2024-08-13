use std::time::Duration;

use anyhow::{Context, Error, Result};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::types::Json;
use sqlx::{ConnectOptions, Pool, Postgres};
use tracing::log::LevelFilter;
use tracing::{event, Level};

use crate::api_client::api_model::RegistrationResponse;
use crate::configuration::AgentConfiguration;
use crate::st_client::Data;
use crate::st_model::StStatusResponse;

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

pub(crate) async fn load_registration(
    pool: &Pool<Postgres>,
) -> Result<Option<DbRegistrationResponse>> {
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

pub(crate) async fn save_registration(
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
