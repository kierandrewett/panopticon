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

fn state_file_path() -> std::path::PathBuf {
    let dir = std::env::var("DATA_DIR").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(dir).join("state.json")
}

#[derive(Serialize, Deserialize, Clone)]
struct DeviceState {
    status: i8,
    since: chrono::DateTime<chrono::Utc>,
    /// Seconds after `since` an active ping is considered stale. None = never.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    ttl_seconds: Option<i64>,
}

#[derive(Serialize, Deserialize, Default)]
struct State {
    #[serde(default)]
    devices: HashMap<String, DeviceState>,
}

/// Legacy on-disk format before per-device tracking. Migrated into `default`.
#[derive(Deserialize)]
struct LegacyState {
    status: i8,
    since: chrono::DateTime<chrono::Utc>,
}

fn load_state() -> State {
    let Ok(raw) = std::fs::read_to_string(state_file_path()) else {
        return State::default();
    };

    if let Ok(state) = serde_json::from_str::<State>(&raw) {
        if !state.devices.is_empty() {
            return state;
        }
    }

    if let Ok(legacy) = serde_json::from_str::<LegacyState>(&raw) {
        let mut devices = HashMap::new();
        devices.insert(
            DEFAULT_DEVICE.to_string(),
            DeviceState {
                status: legacy.status,
                since: legacy.since,
                ttl_seconds: None,
            },
        );
        tracing::info!("Migrated legacy state.json into device '{DEFAULT_DEVICE}'");
        return State { devices };
    }

    State::default()
}

fn save_state(state: &State) {
    if let Err(e) = std::fs::write(state_file_path(), serde_json::to_string(state).unwrap()) {
        tracing::error!("Failed to save state: {}", e);
    }
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(load_state()));

#[derive(Deserialize)]
struct UpdateRequest {
    status: i8,
    #[serde(default)]
    device: Option<String>,
    /// Optional TTL in seconds. After this many seconds an active ping
    /// auto-decays to idle. Applies to this ping (persists for the device).
    #[serde(default)]
    ttl: Option<i64>,
}

#[derive(Serialize)]
struct DeviceStatusResponse {
    status: String,
    since: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stale: Option<bool>,
}

#[derive(Serialize)]
struct StatusResponse {
    status: String,
    since: String,
    devices: HashMap<String, DeviceStatusResponse>,
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

/// True if a device is currently effectively active (status=1 and not past TTL).
fn device_effective_active(d: &DeviceState, now: chrono::DateTime<chrono::Utc>) -> bool {
    if d.status != 1 {
        return false;
    }
    match d.ttl_seconds {
        Some(ttl) => (now - d.since).num_seconds() < ttl,
        None => true,
    }
}

/// The instant a device most recently became (or will be reported as) idle.
/// For an active, unexpired device this returns `since` (it isn't idle).
fn device_idle_instant(
    d: &DeviceState,
    now: chrono::DateTime<chrono::Utc>,
) -> chrono::DateTime<chrono::Utc> {
    if d.status == 0 {
        return d.since;
    }
    match d.ttl_seconds {
        Some(ttl) => {
            let expiry = d.since + chrono::Duration::seconds(ttl);
            if expiry <= now { expiry } else { now }
        }
        None => now,
    }
}

/// Returns (aggregate_status, aggregate_since).
/// Active if any device is effectively active; "since" is the earliest such
/// device's `since`. When idle, "since" is the most recent moment any device
/// transitioned (or expired) to idle.
fn compute_aggregate(
    devices: &HashMap<String, DeviceState>,
    now: chrono::DateTime<chrono::Utc>,
) -> (i8, chrono::DateTime<chrono::Utc>) {
    let active_sinces: Vec<chrono::DateTime<chrono::Utc>> = devices
        .values()
        .filter(|d| device_effective_active(d, now))
        .map(|d| d.since)
        .collect();

    if let Some(earliest) = active_sinces.iter().min() {
        return (1, *earliest);
    }

    let idle_since = devices
        .values()
        .map(|d| device_idle_instant(d, now))
        .max()
        .unwrap_or(now);
    (0, idle_since)
}

fn parse_update_request(headers: &axum::http::HeaderMap, body: &str) -> Result<UpdateRequest, ()> {
    let content_type = headers
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if content_type.contains("application/json") {
        serde_json::from_str::<UpdateRequest>(body).map_err(|_| ())
    } else {
        // Plain-text fallback: body is just "0" or "1", no device/ttl.
        let status = body.trim().parse::<i8>().map_err(|_| ())?;
        Ok(UpdateRequest {
            status,
            device: None,
            ttl: None,
        })
    }
}

async fn update_status(headers: axum::http::HeaderMap, body: String) -> impl IntoResponse {
    let auth = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if auth != format!("Bearer {}", get_token()) {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let req = match parse_update_request(&headers, &body) {
        Ok(r) if r.status == 0 || r.status == 1 => r,
        _ => return (axum::http::StatusCode::BAD_REQUEST, "Invalid status").into_response(),
    };

    let device_id = req
        .device
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_DEVICE)
        .to_string();

    let mut state = STATE.lock().await;
    let now = chrono::Utc::now();
    let existing = state.devices.get(&device_id);

    let status_changed = existing.map(|d| d.status != req.status).unwrap_or(true);
    let ttl_changed = existing
        .map(|d| d.ttl_seconds != req.ttl)
        .unwrap_or(req.ttl.is_some());

    let new_device = DeviceState {
        status: req.status,
        since: if status_changed {
            now
        } else {
            existing.unwrap().since
        },
        ttl_seconds: req.ttl,
    };

    if status_changed || ttl_changed {
        state.devices.insert(device_id.clone(), new_device);
        save_state(&state);
        tracing::info!(
            "Device '{}' -> status {} (ttl {:?})",
            device_id,
            req.status,
            req.ttl
        );
    } else {
        // No-op refresh of an unchanged active ping.
        state.devices.insert(device_id, new_device);
    }

    "ok".into_response()
}

#[derive(Deserialize, Default)]
struct GetParams {
    #[serde(default)]
    format: Option<String>,
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

    let state = STATE.lock().await;
    let now = chrono::Utc::now();
    let (agg_status, agg_since) = compute_aggregate(&state.devices, now);

    let result = match agg_status {
        0 => "no",
        1 => "yes",
        _ => "invalid status",
    };
    let since = format_since(agg_since);

    match format {
        "json" => {
            let devices = state
                .devices
                .iter()
                .map(|(name, d)| {
                    let active = device_effective_active(d, now);
                    let stale = d.status == 1 && !active;
                    (
                        name.clone(),
                        DeviceStatusResponse {
                            status: if active { "yes" } else { "no" }.to_string(),
                            since: format_since(d.since),
                            ttl_seconds: d.ttl_seconds,
                            stale: if stale { Some(true) } else { None },
                        },
                    )
                })
                .collect();

            Json(StatusResponse {
                status: result.to_string(),
                since,
                devices,
            })
            .into_response()
        }
        "text" => result.to_string().into_response(),
        _ => {
            let mut device_lines: Vec<(String, String)> = state
                .devices
                .iter()
                .map(|(name, d)| {
                    let active = device_effective_active(d, now);
                    let label = if active {
                        "active"
                    } else if d.status == 1 {
                        "stale"
                    } else {
                        "idle"
                    };
                    (name.clone(), format!("{label} since {}", format_since(d.since)))
                })
                .collect();
            device_lines.sort_by(|a, b| a.0.cmp(&b.0));

            let devices_html = if device_lines.is_empty() {
                String::new()
            } else {
                let items: String = device_lines
                    .into_iter()
                    .map(|(name, line)| {
                        format!(
                            "<li><strong>{}</strong>: {}</li>",
                            html_escape(&name),
                            html_escape(&line)
                        )
                    })
                    .collect();
                format!("<ul class=\"devices\">{items}</ul>")
            };

            Html(
                TEMPLATE_STR
                    .replace("{{result}}", result)
                    .replace("{{since}}", &since)
                    .replace("{{devices}}", &devices_html),
            )
            .into_response()
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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

    // build our application with a single route
    let app = Router::new().route("/", post(update_status).get(get_status));

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind(&format!("0.0.0.0:{}", port))
        .await
        .unwrap();

    tracing::info!("Listening on http://localhost:{}", port);

    axum::serve(listener, app).await.unwrap();
}
