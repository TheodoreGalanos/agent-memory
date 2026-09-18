use crate::{Host, map_error};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use memory_domain::{
    contracts::{ContractError, ReasonCode},
    coordination::{HostRequest, HostResponse},
};
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
struct Api {
    host: Host,
    requests: Arc<Semaphore>,
}

pub fn router(host: Host) -> Router {
    Router::new()
        .route("/v1/commands", post(command))
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .route("/openapi.json", get(openapi))
        .route("/schemas/{name}", get(schema))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .route(
            "/v1/artifacts/{id}",
            get(read_artifact)
                .put(upload_artifact)
                .layer(DefaultBodyLimit::max(16 * 1024 * 1024)),
        )
        .with_state(Api {
            host,
            requests: Arc::new(Semaphore::new(32)),
        })
}
async fn command(
    State(api): State<Api>,
    headers: HeaderMap,
    Json(request): Json<HostRequest>,
) -> Result<Json<HostResponse>, (StatusCode, Json<ContractError>)> {
    let _permit = api.requests.try_acquire().map_err(|_| {
        (
            StatusCode::TOO_MANY_REQUESTS,
            Json(map_error(memory_store::Error::LimitExceeded)),
        )
    })?;
    api.host
        .handle(
            headers.get("authorization").and_then(|v| v.to_str().ok()),
            request,
        )
        .await
        .map(Json)
        .map_err(api_error)
}
fn api_error(error: ContractError) -> (StatusCode, Json<ContractError>) {
    let status = match error.code {
        ReasonCode::Unauthenticated => StatusCode::UNAUTHORIZED,
        ReasonCode::ForbiddenScope => StatusCode::FORBIDDEN,
        ReasonCode::SourceUnavailable => StatusCode::NOT_FOUND,
        ReasonCode::RequestConflict => StatusCode::CONFLICT,
        ReasonCode::BudgetExhausted => StatusCode::TOO_MANY_REQUESTS,
        ReasonCode::InvalidPayload => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(error))
}
async fn upload_artifact(
    State(api): State<Api>,
    Path(id): Path<uuid::Uuid>,
    headers: HeaderMap,
    bytes: axum::body::Bytes,
) -> Result<Json<memory_domain::sources::Artifact>, (StatusCode, Json<ContractError>)> {
    let _permit = api
        .requests
        .try_acquire()
        .map_err(|_| api_error(map_error(memory_store::Error::LimitExceeded)))?;
    api.host
        .upload_artifact(
            headers.get("authorization").and_then(|v| v.to_str().ok()),
            id,
            &bytes,
        )
        .await
        .map(Json)
        .map_err(api_error)
}
#[derive(serde::Deserialize)]
struct ArtifactRange {
    offset: u32,
    limit: u32,
}
async fn read_artifact(
    State(api): State<Api>,
    Path(id): Path<uuid::Uuid>,
    Query(range): Query<ArtifactRange>,
    headers: HeaderMap,
) -> Result<Vec<u8>, (StatusCode, Json<ContractError>)> {
    let _permit = api
        .requests
        .try_acquire()
        .map_err(|_| api_error(map_error(memory_store::Error::LimitExceeded)))?;
    api.host
        .read_artifact(
            headers.get("authorization").and_then(|v| v.to_str().ok()),
            id,
            range.offset,
            range.limit,
        )
        .await
        .map_err(api_error)
}

async fn schema(Path(name): Path<String>) -> Result<Json<serde_json::Value>, StatusCode> {
    let raw = match name.as_str() {
        "host-request.json" => {
            include_str!("../../../contracts/generated/host-request.schema.json")
        }
        "host-response.json" => {
            include_str!("../../../contracts/generated/host-response.schema.json")
        }
        _ => return Err(StatusCode::NOT_FOUND),
    };
    serde_json::from_str(raw)
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
async fn openapi() -> Json<serde_json::Value> {
    let mut document = serde_json::json!({
        "openapi": "3.1.0", "info": {"title":"Agent Memory Host", "version":"0.1.0"},
        "components": {"securitySchemes":{"bearer":{"type":"http","scheme":"bearer"}}},
        "paths": {"/v1/commands":{"post":{
            "operationId":"hostCommand", "security":[{"bearer":[]}],
            "requestBody":{"required":true,"content":{"application/json":{"schema":{"$ref":"/schemas/host-request.json"}}}},
            "responses":{
                "200":{"description":"Command result","content":{"application/json":{"schema":{"$ref":"/schemas/host-response.json"}}}},
                "400":{"description":"Invalid command"},"401":{"description":"Authentication required"},
                "403":{"description":"Assignment or scope denied"},"404":{"description":"Scoped resource unavailable"},
                "409":{"description":"Request, revision or ownership conflict"},"413":{"description":"Request exceeds 2 MiB"},
                "429":{"description":"Budget or concurrent request limit"},"500":{"description":"Internal failure"}
            }
        }}}
    });
    document["paths"]["/v1/artifacts/{id}"] = serde_json::json!({
        "parameters": [{"name":"id","in":"path","required":true,"schema":{"type":"string","format":"uuid"}}],
        "put": {"operationId":"publishArtifact","security":[{"bearer":[]}],"description":"Administrator publication of an allocated artifact; bounded to 16 MiB.","requestBody":{"required":true,"content":{"application/octet-stream":{"schema":{"type":"string","format":"binary"}}}},"responses":{"200":{"description":"Ready artifact metadata"},"401":{"description":"Authentication required"},"403":{"description":"Administrator required"},"409":{"description":"Artifact already published"},"413":{"description":"Upload exceeds 16 MiB"}}},
        "get": {"operationId":"readArtifact","security":[{"bearer":[]}],"parameters":[{"name":"offset","in":"query","required":true,"schema":{"type":"integer","minimum":0}},{"name":"limit","in":"query","required":true,"schema":{"type":"integer","minimum":0,"maximum":1048576}}],"responses":{"200":{"description":"Scoped artifact byte range","content":{"application/octet-stream":{"schema":{"type":"string","format":"binary"}}}},"403":{"description":"Scope denied"}}}
    });
    Json(document)
}

async fn healthz() -> &'static str {
    "ok\n"
}
async fn readyz(State(api): State<Api>) -> (StatusCode, String) {
    match api.host.ready().await {
        Ok(()) => (StatusCode::OK, "ready: database reachable\n".into()),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "not ready: database unreachable\n".into(),
        ),
    }
}
/// Prometheus text format; counts only, no identifiers.
async fn metrics(State(api): State<Api>) -> (StatusCode, String) {
    let Ok(operations) = api.host.operations().await else {
        return (StatusCode::SERVICE_UNAVAILABLE, String::new());
    };
    let mut out = String::new();
    out.push_str("# TYPE memory_host_uptime_seconds gauge\n");
    out.push_str(&format!(
        "memory_host_uptime_seconds {}\n",
        operations.uptime_seconds
    ));
    out.push_str("# TYPE memory_issued_credentials gauge\n");
    out.push_str(&format!(
        "memory_issued_credentials {}\n",
        operations.issued_worker_credentials
    ));
    out.push_str("# TYPE memory_jobs_total gauge\n");
    for state in [
        "queued",
        "leased",
        "running",
        "waiting",
        "completed",
        "partial",
        "failed",
        "cancelled",
    ] {
        let count = operations
            .jobs
            .iter()
            .find(|(s, _)| s == state)
            .map(|(_, c)| *c)
            .unwrap_or(0);
        out.push_str(&format!("memory_jobs_total{{state=\"{state}\"}} {count}\n"));
    }
    (StatusCode::OK, out)
}
