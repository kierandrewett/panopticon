use axum::{
    Router,
    response::{Html, IntoResponse, Json},
    routing::post,
};
use chrono::Datelike;
use chrono_tz::Europe::London;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use tokio::sync::Mutex;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

const DISPLAY_TZ_NAME: &str = "Europe/London";

fn state_file_path() -> std::path::PathBuf {
    let dir = std::env::var("DATA_DIR").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(dir).join("state.json")
}

#[derive(Serialize, Deserialize)]
struct State {
    status: i8,
    since: chrono::DateTime<chrono::Utc>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            status: 0,
            since: chrono::Utc::now(),
        }
    }
}

fn load_state() -> State {
    std::fs::read_to_string(state_file_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
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

fn parse_status_value(headers: &axum::http::HeaderMap, body: &str) -> Result<i8, ()> {
    let content_type = headers
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if content_type.contains("application/json") {
        serde_json::from_str::<UpdateRequest>(body)
            .map(|r| r.status)
            .map_err(|_| ())
    } else {
        body.trim().parse::<i8>().map_err(|_| ())
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

    let new_status = match parse_status_value(&headers, &body) {
        Ok(s @ 0) | Ok(s @ 1) => s,
        _ => return (axum::http::StatusCode::BAD_REQUEST, "Invalid status").into_response(),
    };

    let mut state = STATE.lock().await;
    if state.status != new_status {
        state.status = new_status;
        state.since = chrono::Utc::now();
        save_state(&state);
        tracing::info!("Status changed to {}", new_status);
    }

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

    let state = STATE.lock().await;

    let result = match state.status {
        0 => "no",
        1 => "yes",
        _ => "invalid status",
    };

    let dt = state.since.with_timezone(&London);
    let since = format!(
        "{}{} {} ({})",
        dt.day(),
        ordinal_suffix(dt.day()),
        dt.format("%b %Y %H:%M:%S"),
        DISPLAY_TZ_NAME,
    )
    .to_string();

    match format {
        "json" => Json(StatusResponse {
            status: result.to_string(),
            since: since.clone(),
        })
        .into_response(),
        "text" => result.to_string().into_response(),
        _ => Html(TEMPLATE_STR.replace("{{result}}", result).replace("{{since}}", &since)).into_response(),
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

    // build our application with a single route
    let app = Router::new().route("/", post(update_status).get(get_status));

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind(&format!("0.0.0.0:{}", port))
        .await
        .unwrap();

    tracing::info!("Listening on http://localhost:{}", port);

    axum::serve(listener, app).await.unwrap();
}
