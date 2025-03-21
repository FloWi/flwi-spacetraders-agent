use std::any::type_name;
use std::future::Future;

use anyhow::Result;
use chrono::{DateTime, Local, Utc};
use futures::future::{self, TryFutureExt};
use serde::Deserialize;
use tracing::log::trace;
use tracing::{event, trace_span, Instrument, Level};

#[derive(Debug, Clone)]
pub struct PaginationInput {
    pub page: u32,
    pub limit: u32,
}

#[derive(Deserialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    meta: Meta,
}

#[derive(Deserialize)]
struct Meta {
    total: u64,
    page: u64,
    limit: u64,
}

pub async fn fetch_all_pages<T, F, Fut>(mut fetch_page: F) -> Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
    F: FnMut(PaginationInput) -> Fut,
    Fut: Future<Output = Result<PaginatedResponse<T>>>,
{
    let initial_input = PaginationInput { page: 1, limit: 20 };

    let mut all_data = Vec::new();
    let mut current_input = initial_input;

    let output_parameter_type_name = type_name::<T>();

    let span = trace_span!("pagination");

    let mut total_number_of_pages = 1;

    async move {
        event!(Level::TRACE, "Start downloading all pages of type {}", output_parameter_type_name);

        while current_input.page <= total_number_of_pages {
            let response = fetch_page(current_input.clone()).await?;
            total_number_of_pages = (response.meta.total as f32 / response.meta.limit as f32).ceil() as u32;

            event!(Level::TRACE, "Downloaded page {} of {}", current_input.page, total_number_of_pages);

            all_data.extend(response.data);

            current_input.page += 1;
        }

        event!(Level::TRACE, "Done downloading all {} pages", total_number_of_pages);
        Ok(all_data)
    }
    .instrument(span)
    .await
}

pub async fn fetch_all_pages_into_queue<T, F, Fut>(
    mut fetch_page: F,
    initial_input: PaginationInput,
    tx: tokio::sync::mpsc::Sender<(Vec<T>, DateTime<Utc>)>,
) -> Result<()>
where
    T: for<'de> Deserialize<'de> + Send + Sync + 'static,
    F: FnMut(PaginationInput) -> Fut,
    Fut: Future<Output = Result<PaginatedResponse<T>>>,
{
    let mut current_input = initial_input;
    let output_parameter_type_name = type_name::<T>();
    let span = tracing::span!(Level::TRACE, "pagination");

    let mut total_number_of_pages = 1;

    async {
        event!(Level::TRACE, "Start downloading all pages of type {}", output_parameter_type_name);

        while current_input.page <= total_number_of_pages {
            let now = Local::now().to_utc();
            let response = fetch_page(current_input.clone()).await?;
            total_number_of_pages = (response.meta.total as f32 / response.meta.limit as f32).ceil() as u32;

            event!(Level::TRACE, "Downloaded page {} of {}", current_input.page, total_number_of_pages);

            tx.send((response.data, now)).await.map_err(|e| anyhow::anyhow!("Failed to send data: {}", e))?;
            current_input.page += 1;
        }

        event!(Level::TRACE, "Done downloading all {} pages", total_number_of_pages);
        Ok(())
    }
    .instrument(span)
    .await
}

pub async fn collect_results<T, U, F, Fut>(collection: impl IntoIterator<Item = T>, f: F) -> Result<Vec<U>>
where
    F: Fn(T) -> Fut + Clone, // Add Clone bound here
    Fut: Future<Output = Result<U>>,
    T: std::fmt::Debug,
    U: std::fmt::Debug,
{
    let collection: Vec<T> = collection.into_iter().collect();
    let total = collection.len();

    let input_parameter_type_name = type_name::<T>();
    let output_parameter_type_name = type_name::<U>();

    let span = trace_span!("collect_results",);

    async move {
        trace!(
            "Start processing all {} items of type {} to collect type Vec<{}>",
            total,
            input_parameter_type_name,
            output_parameter_type_name
        );
        let results = future::try_join_all(collection.into_iter().enumerate().map(move |(index, item)| {
            // Use move here
            let f = f.clone(); // Clone f for each iteration
            async move {
                trace!("Processing item {} of {} {:?}", index + 1, total, item);
                let result = f(item).await;
                trace!("Finished processing item {} of {}", index + 1, total);
                result
            }
        }))
        .await?;

        trace!(
            "Finished processing all {} items of type {} to collect type Vec<{}>",
            total,
            input_parameter_type_name,
            output_parameter_type_name
        );

        Ok(results)
    }
    .instrument(span)
    .await
}
