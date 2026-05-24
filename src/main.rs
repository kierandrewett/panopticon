use axum::{
    Router,
    response::{Html, IntoResponse, Json},
    routing::post,
};
use chrono::Datelike;
use chrono_tz::Europe::London;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;
use tokio::sync::Mutex;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

const DISPLAY_TZ_NAME: &str = "Europe/London";
const DEFAULT_DEVICE: &str = "default";
/// Devices not seen for this long are pruned and stop contributing to the
/// aggregate. Expected client ping cadence is well under this (e.g. 30s).
const DEVICE_TTL_SECONDS: i64 = 90;

fn state_file_path() -> std::path::PathBuf {
    let dir = std::env::var("DATA_DIR").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(dir).join("state.json")
}

#[derive(Serialize, Deserialize, Clone)]
struct DeviceState {
    last_seen: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Default)]
struct State {
    #[serde(default)]
    devices: HashMap<String, DeviceState>,
    /// True at the moment of the last save. Compared to the freshly-computed
    /// state on each request to detect active <-> inactive transitions.
    #[serde(default)]
    aggregate_active: bool,
    /// When the aggregate active/inactive state last flipped.
    #[serde(default)]
    aggregate_since: Option<chrono::DateTime<chrono::Utc>>,
}

fn load_state() -> State {
    let Ok(raw) = std::fs::read_to_string(state_file_path()) else {
        return State::default();
    };
    serde_json::from_str::<State>(&raw).unwrap_or_default()
}

fn save_state(state: &State) {
    if let Err(e) = std::fs::write(state_file_path(), serde_json::to_string(state).unwrap()) {
        tracing::error!("Failed to save state: {}", e);
    }
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(load_state()));

#[derive(Deserialize, Default)]
struct UpdateRequest {
    /// 1 (or omitted) = device is here. 0 = remove this device immediately.
    #[serde(default)]
    status: Option<i8>,
    #[serde(default)]
    device: Option<String>,
}

#[derive(Deserialize, Default)]
struct UpdateQuery {
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    status: Option<i8>,
}

#[derive(Serialize)]
struct StatusResponse {
    status: String,
    since: String,
}

const TEMPLATE_STR: &'static str = include_str!("../template.html");

fn get_token() -> String {
    std::env::var("TOKEN").expect("No TOKEN in env")
}

fn ordinal_suffix(day: u32) -> &'static str {
    match day % 100 {
        11 | 12 | 13 => "th",
        _ => match day % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    }
}

fn format_since(dt: chrono::DateTime<chrono::Utc>) -> String {
    let dt = dt.with_timezone(&London);
    format!(
        "{}{} {} ({})",
        dt.day(),
        ordinal_suffix(dt.day()),
        dt.format("%b %Y %H:%M:%S"),
        DISPLAY_TZ_NAME,
    )
}

/// Drop devices whose `last_seen` is older than the TTL. Returns the latest
/// expiry instant among pruned devices (useful for setting aggregate_since).
fn prune_devices(
    devices: &mut HashMap<String, DeviceState>,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    let mut latest_expiry: Option<chrono::DateTime<chrono::Utc>> = None;
    devices.retain(|_, d| {
        let alive = (now - d.last_seen).num_seconds() < DEVICE_TTL_SECONDS;
        if !alive {
            let expiry = d.last_seen + chrono::Duration::seconds(DEVICE_TTL_SECONDS);
            latest_expiry = latest_expiry.map(|t| t.max(expiry)).or(Some(expiry));
        }
        alive
    });
    latest_expiry
}

/// Prune, then return aggregate (active, since). Updates persisted
/// `aggregate_since` only when the active/inactive state flips.
fn refresh_aggregate(
    state: &mut State,
    now: chrono::DateTime<chrono::Utc>,
) -> (bool, chrono::DateTime<chrono::Utc>) {
    let was_active = state.aggregate_active;
    let latest_expiry = prune_devices(&mut state.devices, now);
    let is_active = !state.devices.is_empty();

    if was_active != is_active {
        let transition = if is_active {
            state
                .devices
                .values()
                .map(|d| d.last_seen)
                .min()
                .unwrap_or(now)
        } else {
            latest_expiry.unwrap_or(now)
        };
        state.aggregate_since = Some(transition);
        state.aggregate_active = is_active;
    } else if state.aggregate_since.is_none() {
        state.aggregate_since = Some(now);
        state.aggregate_active = is_active;
    }

    (is_active, state.aggregate_since.unwrap())
}

fn parse_update(
    headers: &axum::http::HeaderMap,
    query: &UpdateQuery,
    body: &str,
) -> Result<UpdateRequest, ()> {
    let content_type = headers
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let mut req = if content_type.contains("application/json") && !body.trim().is_empty() {
        serde_json::from_str::<UpdateRequest>(body).map_err(|_| ())?
    } else if !body.trim().is_empty() {
        // Plain-text body: "0" or "1" sets status; anything else is treated
        // as a device name (e.g. iPhone Shortcut posting raw text).
        let trimmed = body.trim();
        match trimmed.parse::<i8>() {
            Ok(n) => UpdateRequest {
                status: Some(n),
                device: None,
            },
            Err(_) => UpdateRequest {
                status: None,
                device: Some(trimmed.to_string()),
            },
        }
    } else {
        UpdateRequest::default()
    };

    if req.device.is_none() {
        req.device = query.device.clone();
    }
    if req.status.is_none() {
        req.status = query.status;
    }

    Ok(req)
}

async fn update_status(
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<UpdateQuery>,
    body: String,
) -> impl IntoResponse {
    let auth = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if auth != format!("Bearer {}", get_token()) {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let req = match parse_update(&headers, &query, &body) {
        Ok(r) => r,
        Err(_) => return (axum::http::StatusCode::BAD_REQUEST, "Invalid body").into_response(),
    };

    let status = req.status.unwrap_or(1);
    if status != 0 && status != 1 {
        return (axum::http::StatusCode::BAD_REQUEST, "Invalid status").into_response();
    }

    let device_id = req
        .device
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_DEVICE)
        .to_string();

    let mut state = STATE.lock().await;
    let now = chrono::Utc::now();

    if status == 0 {
        if state.devices.remove(&device_id).is_some() {
            tracing::info!("Device '{}' delisted", device_id);
        }
    } else {
        state
            .devices
            .insert(device_id.clone(), DeviceState { last_seen: now });
        tracing::debug!("Device '{}' refreshed", device_id);
    }

    refresh_aggregate(&mut state, now);
    save_state(&state);

    "ok".into_response()
}

#[derive(Deserialize, Default)]
struct GetParams {
    #[serde(default)]
    format: Option<String>,
}

async fn get_status(
    headers: axum::http::HeaderMap,
    axum::extract::Query(params): axum::extract::Query<GetParams>,
) -> impl IntoResponse {
    let ua = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    let accept = headers
        .get("accept")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let format = match params.format.as_deref() {
        Some("json") => "json",
        Some("html") => "html",
        Some("text") => "text",
        _ if accept.contains("application/json") => "json",
        _ if ua.contains("curl") => "text",
        _ => "html",
    };

    let mut state = STATE.lock().await;
    let now = chrono::Utc::now();
    let prev_active = state.aggregate_active;
    let (active, since_dt) = refresh_aggregate(&mut state, now);
    if prev_active != active {
        save_state(&state);
    }

    let result = if active { "yes" } else { "no" };
    let since = format_since(since_dt);

    match format {
        "json" => Json(StatusResponse {
            status: result.to_string(),
            since,
        })
        .into_response(),
        "text" => result.to_string().into_response(),
        _ => Html(
            TEMPLATE_STR
                .replace("{{result}}", result)
                .replace("{{since}}", &since),
        )
        .into_response(),
    }
}

#[tokio::main]
async fn main() {
    tracing::subscriber::set_global_default(
        FmtSubscriber::builder()
            .with_max_level(Level::INFO)
            .finish(),
    )
    .expect("tracing setup failed");

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());

    let _ = get_token(); // Ensure TOKEN is set at startup

    let app = Router::new().route("/", post(update_status).get(get_status));

    let listener = tokio::net::TcpListener::bind(&format!("0.0.0.0:{}", port))
        .await
        .unwrap();

    tracing::info!("Listening on http://localhost:{}", port);

    axum::serve(listener, app).await.unwrap();
}
