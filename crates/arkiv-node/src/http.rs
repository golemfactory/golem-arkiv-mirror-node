//! Request plumbing shared by the signed and simulator HTTP runtimes.

use crate::error::{Error, ErrorCode, Result};
use crate::simulator::wire::json_bytes;
use axum::{
    body::{Body, to_bytes},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value as Json;
use std::{sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// One admission slot, or `ServerBusy` without waiting.
pub(crate) fn admit(slots: &Arc<Semaphore>) -> Result<OwnedSemaphorePermit> {
    slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ErrorCode::ServerBusy.into())
}

/// The JSON body of a request, read within `cap` bytes and five seconds.
pub(crate) async fn json_body<T: serde::de::DeserializeOwned>(body: Body, cap: usize) -> Result<T> {
    let bytes = tokio::time::timeout(Duration::from_secs(5), to_bytes(body, cap))
        .await
        .map_err(|_| ErrorCode::InvalidRequest)?
        .map_err(|_| ErrorCode::InvalidRequest)?;
    serde_json::from_slice(&bytes).map_err(|_| Error::from(ErrorCode::InvalidRequest))
}

/// The response to `result`: its JSON within `cap` bytes, or the refusal
/// under the runtime's `status` for its code.
pub(crate) fn json_response(
    result: Result<Json>,
    cap: usize,
    status: fn(ErrorCode) -> StatusCode,
) -> Response {
    match result.and_then(|value| json_bytes(&value, cap)) {
        Ok(bytes) => (
            StatusCode::OK,
            [("content-type", "application/json")],
            bytes,
        )
            .into_response(),
        Err(e) => {
            e.log_storage_failure();
            (status(e.code), axum::Json(crate::retention::error_body(&e))).into_response()
        }
    }
}
