use axum::error_handling::HandleErrorLayer;
use axum::{extract, routing::post, Json, Router};
use cut_optimizer_1d::{CutPiece, Optimizer, Solution, StockPiece};
use http::{Method, StatusCode, Uri};
use hyper::Body;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::oneshot;
use tower::{BoxError, ServiceBuilder};
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;
use tracing::error;

use crate::Opt;

#[cfg(test)]
mod tests;

/// Run optimizer server
pub(crate) async fn serve(socket_addr: SocketAddr, opt: &Opt) {
    // run it with hyper on localhost:3000
    hyper::Server::bind(&socket_addr)
        .serve(app(opt).into_make_service())
        .await
        .unwrap();
}

fn app(opt: &Opt) -> Router<Body> {
    let middleware_stack = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(handle_error))
        // Return an error after 30 seconds
        .timeout(Duration::from_secs(opt.timeout))
        // Shed load if we're receiving too many requests
        .load_shed()
        // Process at most 100 requests concurrently
        .concurrency_limit(opt.max_requests)
        // Tracing
        .layer(TraceLayer::new_for_http())
        // Compress response bodies
        .layer(CompressionLayer::new());

    Router::new()
        .route("/optimize", post(optimize))
        .layer(middleware_stack)
}

/// Run optimizer in a thread pool
async fn optimize(
    extract::Json(payload): extract::Json<OptimizerInput>,
) -> Result<Json<Solution>, OptimizeError> {
    let (tx, rx) = oneshot::channel();

    rayon::spawn(move || {
        let optimizer: Optimizer = payload.into();
        let result = optimizer.optimize(|_| {});
        if tx.send(result).is_err() {
            error!("Error: receiver side of channel closed before the result could be sent.");
        }
    });

    let result = rx.await.map_err(|e| {
        error_with_data(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Couldn't receive result from channel",
            e.to_string(),
        )
    })?;

    let solution = result.map_err(|e| match e {
        cut_optimizer_1d::Error::NoFitForCutPiece(cut_piece) => error_with_data(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Cut piece doesn't fit in any stock pieces",
            cut_piece,
        ),
    })?;

    Ok(Json(solution))
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OptimizerInput {
    random_seed: Option<u64>,
    cut_width: usize,
    stock_pieces: Vec<StockPiece>,
    cut_pieces: Vec<CutPiece>,
    allow_mixed_stock_sizes: Option<bool>,
}

impl From<OptimizerInput> for Optimizer {
    fn from(input: OptimizerInput) -> Self {
        let mut optimizer = Optimizer::new();
        optimizer
            .set_random_seed(input.random_seed.unwrap_or(1))
            .set_cut_width(input.cut_width)
            .add_stock_pieces(input.stock_pieces)
            .add_cut_pieces(input.cut_pieces)
            .allow_mixed_stock_sizes(input.allow_mixed_stock_sizes.unwrap_or(true));
        optimizer
    }
}

async fn handle_error(method: Method, uri: Uri, err: BoxError) -> OptimizeError {
    if err.is::<tower::timeout::error::Elapsed>() {
        error(StatusCode::REQUEST_TIMEOUT, "Request took too long")
    } else {
        error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("`{} {}` failed with {}", method, uri, err),
        )
    }
}

type OptimizeError = (StatusCode, Json<Value>);

fn error(status_code: StatusCode, message: &str) -> OptimizeError {
    (status_code, Json(json!({ "message": message })))
}

fn error_with_data<T: Serialize>(status_code: StatusCode, message: &str, data: T) -> OptimizeError {
    (
        status_code,
        Json(json!({ "message": message, "data": data })),
    )
}
