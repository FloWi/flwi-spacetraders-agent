use std::future::Future;

use futures::future::join_all;
use tracing::{event, span, Level};

use crate::st_model::GetMeta;

pub struct PaginationInput {
    pub(crate) page: u32,
    pub(crate) limit: u32,
}

pub async fn paginate<F, T: GetMeta>(call: impl Fn(PaginationInput) -> F) -> Vec<T>
where
    F: Future<Output = T>,
{
    let span = span!(Level::TRACE, "pagination");
    let _ = span.enter();

    event!(Level::TRACE, "Start downloading all pages");

    let first_page = call(PaginationInput { page: 1, limit: 20 }).await;
    let meta = first_page.get_meta();

    let total_number_pages = (meta.total as f32 / meta.limit as f32).ceil() as u32;

    let futures: Vec<_> = (2..=total_number_pages)
        .into_iter()
        .map(|p| {
            event!(Level::TRACE, "Downloading page {p} of {total_number_pages}",);
            call(PaginationInput { page: p, limit: 20 })
        })
        .collect();
    let rest_results: Vec<_> = join_all(futures).await.into_iter().collect();

    event!(
        Level::TRACE,
        "Done downloading rest of {total_number_pages} pages"
    );

    //let futures: Vec<_> = resp.leaderboards.most_credits.iter().map(|a| get_static_agent_info(&client, AgentSymbol(a.agent_symbol.to_string()))).collect();

    // let results: Vec<_> = join_all(futures).await.into_iter().collect();

    let mut result = vec![first_page];
    result.extend(rest_results);
    result
}
