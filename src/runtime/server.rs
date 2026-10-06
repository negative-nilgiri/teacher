use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde::Serialize;
use tokio::net::TcpListener;

use crate::source::NodeId;

use super::model::{
    ArtifactLoadError, QuestionMutationResponse, RunResponse, RuntimeLesson, StateResponse,
    SubmitRequest, load_artifact, project_runtime_lesson,
};
use super::runner::{self, RunResult};
use super::session::{Session, SessionError};

/// The header that carries the per-launch run token. A custom header makes a
/// cross-origin browser request need a CORS preflight, which this server never
/// answers.
const RUN_TOKEN_HEADER: &str = "x-learn-token";

#[derive(Clone)]
struct AppState {
    lesson: Arc<RuntimeLesson>,
    session: Arc<Mutex<Session>>,
    /// The port this server is bound to, which a loopback `Host` must name.
    port: u16,
}

impl AppState {
    /// `run_token` is `Some` exactly when running code is enabled.
    fn new(lesson: RuntimeLesson, run_token: Option<String>, port: u16) -> Self {
        let session = Session::new(&lesson, run_token);
        Self {
            lesson: Arc::new(lesson),
            session: Arc::new(Mutex::new(session)),
            port,
        }
    }
}

/// A server that has loaded its artifact and reserved its loopback port.
pub struct BoundServer {
    listener: TcpListener,
    app: Router,
    address: SocketAddr,
}

impl BoundServer {
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn url(&self) -> String {
        format!("http://{}/", self.address)
    }

    pub async fn run(self) -> Result<(), RuntimeError> {
        axum::serve(self.listener, self.app)
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
            .map_err(RuntimeError::Serve)
    }
}

/// Load one artifact, create one shared in-memory session, and reserve a random
/// IPv4 loopback port. The returned server does not listen beyond loopback.
///
/// With `allow_run`, the learner can run the lesson's `run_code` blocks, which
/// need the random token the state response then carries. Without it no run is
/// possible.
pub async fn bind(
    artifact_path: impl AsRef<Path>,
    allow_run: bool,
) -> Result<BoundServer, RuntimeError> {
    let display_path = artifact_path.as_ref().to_string_lossy().into_owned();
    let artifact = load_artifact(artifact_path).map_err(RuntimeError::Artifact)?;
    let mut lesson = project_runtime_lesson(&artifact);
    lesson.public.artifact_path = Some(display_path);
    let run_token = allow_run.then(new_run_token).transpose()?;
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .map_err(RuntimeError::Bind)?;
    let address = listener.local_addr().map_err(RuntimeError::Bind)?;
    let app = router(AppState::new(lesson, run_token, address.port()));
    Ok(BoundServer {
        listener,
        app,
        address,
    })
}

/// 128 random bits as lowercase hex.
fn new_run_token() -> Result<String, RuntimeError> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(RuntimeError::Token)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/state", get(get_state))
        .route("/api/v1/questions/{node_id}/submit", post(submit))
        .route("/api/v1/questions/{node_id}/reveal", post(reveal))
        .route("/api/v1/runs/{node_id}", post(run_block))
        .fallback(serve_asset)
        .with_state(state)
}

async fn get_state(State(state): State<AppState>) -> Result<Json<StateResponse>, ApiError> {
    let session = state.session.lock().map_err(|_| ApiError::internal())?;
    Ok(Json(StateResponse {
        lesson: state.lesson.public.clone(),
        progress: session.progress(),
        run: session.run_status(),
        runs: session.run_results(),
    }))
}

async fn submit(
    State(state): State<AppState>,
    AxumPath(node_id): AxumPath<u32>,
    Json(request): Json<SubmitRequest>,
) -> Result<Json<QuestionMutationResponse>, ApiError> {
    let mut session = state.session.lock().map_err(|_| ApiError::internal())?;
    let question = session
        .submit(&state.lesson, NodeId::new(node_id), request.choice_id)
        .map_err(ApiError::from_session)?;
    Ok(Json(QuestionMutationResponse {
        progress: session.progress(),
        question,
    }))
}

async fn reveal(
    State(state): State<AppState>,
    AxumPath(node_id): AxumPath<u32>,
) -> Result<Json<QuestionMutationResponse>, ApiError> {
    let mut session = state.session.lock().map_err(|_| ApiError::internal())?;
    let question = session
        .reveal(&state.lesson, NodeId::new(node_id))
        .map_err(ApiError::from_session)?;
    Ok(Json(QuestionMutationResponse {
        progress: session.progress(),
        question,
    }))
}

async fn run_block(
    State(state): State<AppState>,
    AxumPath(node_id): AxumPath<u32>,
    headers: HeaderMap,
    uri: Uri,
) -> Result<Json<RunResponse>, ApiError> {
    require_loopback_host(&headers, &uri, state.port)?;
    let token = headers
        .get(RUN_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok());
    let node_id = NodeId::new(node_id);
    let spec = state
        .session
        .lock()
        .map_err(|_| ApiError::internal())?
        .begin_run(&state.lesson, node_id, token)
        .map_err(ApiError::from_session)?;

    // The run finishes and is recorded even if this request is dropped, so the
    // block is never left marked as running.
    let session = Arc::clone(&state.session);
    let run = tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || runner::run(&spec))
            .await
            .unwrap_or_else(|_| RunResult::failed("the run stopped unexpectedly"));
        if let Ok(mut session) = session.lock() {
            session.finish_run(node_id, result.clone());
        }
        result
    })
    .await
    .map_err(|_| ApiError::internal())?;
    Ok(Json(RunResponse { run }))
}

/// A page that reached this server through another name, such as a rebound DNS
/// name, must not be able to drive it. Only the loopback names with this
/// server's own port are accepted.
fn require_loopback_host(headers: &HeaderMap, uri: &Uri, port: u16) -> Result<(), ApiError> {
    let host = match headers.get(header::HOST) {
        Some(value) => value.to_str().ok(),
        None => uri.authority().map(|authority| authority.as_str()),
    };
    let loopback = host
        .and_then(|host| host.rsplit_once(':'))
        .is_some_and(|(name, given)| {
            (name == "127.0.0.1" || name.eq_ignore_ascii_case("localhost"))
                && given == port.to_string()
        });
    if loopback {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "invalid_host",
            "Runs are only accepted for a loopback Host such as 127.0.0.1 or localhost",
        ))
    }
}

#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct WebAssets;

// The build-script digest makes Cargo recompile this embed when Vite replaces
// content-hashed filenames. rust-embed alone cannot expose newly added paths as
// ordinary Rust source dependencies before the macro runs.
const _: &str = env!("AGENT_TEACHER_WEB_ASSET_DIGEST");

async fn serve_asset(uri: Uri) -> Response {
    if uri.path().starts_with("/api/") {
        return ApiError::not_found("api_route_not_found", "API route not found").into_response();
    }

    let requested = uri.path().trim_start_matches('/');
    let asset_name = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };
    // The lesson UI has no client-side routes, so anything that is not an
    // embedded file is a real miss. Answering it with `index.html` would turn a
    // missing script into a confusing HTML-as-JavaScript error.
    match WebAssets::get(asset_name) {
        Some(asset) => {
            let content_type = mime_guess::from_path(asset_name)
                .first_or_octet_stream()
                .as_ref()
                .to_owned();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(asset.data.into_owned()))
                .expect("static response is valid")
        }
        None => not_found_page(uri.path()),
    }
}

const NOT_FOUND_PAGE: &str = include_str!("not_found.html");

fn not_found_page(path: &str) -> Response {
    let body = NOT_FOUND_PAGE.replace("{{path}}", &escape_html(path));
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(body))
        .expect("static response is valid")
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[derive(Debug)]
pub enum RuntimeError {
    Artifact(ArtifactLoadError),
    Bind(std::io::Error),
    Serve(std::io::Error),
    Token(getrandom::Error),
}

impl RuntimeError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Artifact(error) => error.code(),
            Self::Bind(_) => "server_bind_failed",
            Self::Serve(_) => "server_failed",
            Self::Token(_) => "run_token_failed",
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Artifact(error) => error.fmt(formatter),
            Self::Bind(error) => {
                write!(formatter, "could not bind the local lesson server: {error}")
            }
            Self::Serve(error) => write!(formatter, "local lesson server failed: {error}"),
            Self::Token(error) => {
                write!(formatter, "could not generate the run token: {error}")
            }
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Artifact(error) => Some(error),
            Self::Bind(error) | Self::Serve(error) => Some(error),
            Self::Token(error) => Some(error),
        }
    }
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "session_unavailable",
            message: "The learner session is unavailable".into(),
        }
    }

    fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code,
            message: message.into(),
        }
    }

    fn forbidden(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code,
            message: message.into(),
        }
    }

    fn from_session(error: SessionError) -> Self {
        match error {
            SessionError::QuestionNotFound { node_id } => Self::not_found(
                "question_not_found",
                format!("Question node {node_id} does not exist"),
            ),
            SessionError::ChoiceNotFound { node_id, choice_id } => Self {
                status: StatusCode::BAD_REQUEST,
                code: "choice_not_found",
                message: format!("Choice {choice_id} does not belong to question {node_id}"),
            },
            SessionError::RunDisabled => Self::forbidden(
                "run_disabled",
                "Running code is off; start `learn serve` with --allow-run",
            ),
            SessionError::InvalidToken => Self::forbidden(
                "invalid_token",
                "A run needs the X-Learn-Token header from this launch's state",
            ),
            SessionError::UnknownNode { node_id } => {
                Self::not_found("unknown_node", format!("Node {node_id} does not exist"))
            }
            SessionError::NotRunnable { node_id } => Self::not_found(
                "not_runnable",
                format!("Node {node_id} is not a run_code block"),
            ),
            SessionError::RunInProgress { node_id } => Self {
                status: StatusCode::CONFLICT,
                code: "run_in_progress",
                message: format!("Node {node_id} is already running"),
            },
        }
    }
}

#[derive(Serialize)]
struct ApiErrorBody<'a> {
    code: &'a str,
    message: &'a str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiErrorBody {
                code: self.code,
                message: &self.message,
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::ChoiceId;
    use crate::runtime::model::{
        project_runtime_lesson,
        tests::{quiz_artifact, run_artifact},
    };

    fn state() -> AppState {
        AppState::new(project_runtime_lesson(&quiz_artifact()), None, 4000)
    }

    #[tokio::test]
    async fn state_starts_without_private_quiz_material() {
        let Json(response) = get_state(State(state())).await.unwrap();
        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["progress"]["total_questions"], 1);
        let body = serde_json::to_string(&value).unwrap();
        assert!(!body.contains("correct_choice_id"));
        assert!(!body.contains("explanation"));
    }

    #[tokio::test]
    async fn endpoints_share_attempts_and_reveals_in_one_session() {
        let state = state();
        let Json(wrong) = submit(
            State(state.clone()),
            AxumPath(0),
            Json(SubmitRequest {
                choice_id: ChoiceId::new(0),
            }),
        )
        .await
        .unwrap();
        assert_eq!(wrong.question.attempts.len(), 1);
        assert!(wrong.question.answer.is_none());

        let Json(revealed) = reveal(State(state.clone()), AxumPath(0)).await.unwrap();
        assert!(revealed.question.revealed);
        assert!(revealed.question.answer.is_some());
        let mutation_json = serde_json::to_value(&revealed).unwrap();
        assert_eq!(
            mutation_json["progress"]["questions"]["0"]["revealed"],
            true
        );

        let Json(full) = get_state(State(state)).await.unwrap();
        assert_eq!(full.progress.questions[&NodeId::new(0)].attempts.len(), 1);
        assert!(full.progress.questions[&NodeId::new(0)].revealed);
    }

    #[tokio::test]
    async fn invalid_question_and_choice_have_typed_http_errors() {
        let missing = reveal(State(state()), AxumPath(99)).await.unwrap_err();
        assert_eq!(missing.status, StatusCode::NOT_FOUND);
        assert_eq!(missing.code, "question_not_found");

        let invalid_choice = submit(
            State(state()),
            AxumPath(0),
            Json(SubmitRequest {
                choice_id: ChoiceId::new(99),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(invalid_choice.status, StatusCode::BAD_REQUEST);
        assert_eq!(invalid_choice.code, "choice_not_found");
    }

    const PORT: u16 = 4000;

    fn run_state(token: Option<&str>) -> AppState {
        AppState::new(
            project_runtime_lesson(&run_artifact()),
            token.map(str::to_owned),
            PORT,
        )
    }

    fn run_headers(host: Option<&str>, token: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(host) = host {
            headers.insert(header::HOST, host.parse().unwrap());
        }
        if let Some(token) = token {
            headers.insert(RUN_TOKEN_HEADER, token.parse().unwrap());
        }
        headers
    }

    async fn post_run(
        state: &AppState,
        node: u32,
        host: Option<&str>,
        token: Option<&str>,
    ) -> Result<Json<RunResponse>, ApiError> {
        run_block(
            State(state.clone()),
            AxumPath(node),
            run_headers(host, token),
            "/api/v1/runs/2".parse().unwrap(),
        )
        .await
    }

    #[tokio::test]
    async fn state_reports_whether_running_is_enabled_and_the_token_only_then() {
        let Json(off) = get_state(State(run_state(None))).await.unwrap();
        let off = serde_json::to_value(off).unwrap();
        assert_eq!(
            off["run"],
            serde_json::json!({"enabled": false, "token": null})
        );
        assert_eq!(off["runs"], serde_json::json!({}));

        let Json(on) = get_state(State(run_state(Some("abc")))).await.unwrap();
        let on = serde_json::to_value(on).unwrap();
        assert_eq!(
            on["run"],
            serde_json::json!({"enabled": true, "token": "abc"})
        );
    }

    #[test]
    fn run_tokens_are_128_random_bits_of_hex() {
        let first = new_run_token().unwrap();
        assert_eq!(first.len(), 32);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
        assert_ne!(first, new_run_token().unwrap());
    }

    #[tokio::test]
    async fn run_requests_are_refused_before_anything_runs() {
        let host = Some("127.0.0.1:4000");
        let off = run_state(None);
        let error = post_run(&off, 2, host, Some("abc")).await.unwrap_err();
        assert_eq!(
            (error.status, error.code),
            (StatusCode::FORBIDDEN, "run_disabled")
        );

        let on = run_state(Some("abc"));
        for token in [None, Some("abd")] {
            let error = post_run(&on, 2, host, token).await.unwrap_err();
            assert_eq!(
                (error.status, error.code),
                (StatusCode::FORBIDDEN, "invalid_token")
            );
        }
        for host in [
            None,
            Some("evil.example:4000"),
            Some("127.0.0.1:4001"),
            Some("127.0.0.1"),
            Some("localhost.evil.example:4000"),
            Some("[::1]:4000"),
        ] {
            let error = post_run(&on, 2, host, Some("abc")).await.unwrap_err();
            assert_eq!(
                (error.status, error.code),
                (StatusCode::FORBIDDEN, "invalid_host"),
                "{host:?}"
            );
        }
        for (node, code) in [
            (0, "not_runnable"),
            (1, "not_runnable"),
            (99, "unknown_node"),
        ] {
            let error = post_run(&on, node, host, Some("abc")).await.unwrap_err();
            assert_eq!((error.status, error.code), (StatusCode::NOT_FOUND, code));
        }
        let Json(state) = get_state(State(on)).await.unwrap();
        assert!(state.runs.is_empty());
    }

    #[test]
    fn only_loopback_names_with_the_servers_port_are_accepted() {
        let uri: Uri = "/api/v1/runs/2".parse().unwrap();
        for host in ["127.0.0.1:4000", "localhost:4000", "LocalHost:4000"] {
            assert!(require_loopback_host(&run_headers(Some(host), None), &uri, PORT).is_ok());
        }
        // An HTTP/2 request carries its host in the URI instead.
        let h2: Uri = "http://127.0.0.1:4000/api/v1/runs/2".parse().unwrap();
        assert!(require_loopback_host(&run_headers(None, None), &h2, PORT).is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_run_is_recorded_in_the_session_and_visible_in_state() {
        let state = run_state(Some("abc"));
        let Json(response) = post_run(&state, 2, Some("localhost:4000"), Some("abc"))
            .await
            .unwrap();
        assert_eq!(response.run.stdout, "shown\n");
        assert_eq!(response.run.exit_code, Some(0));
        let body = serde_json::to_value(&response).unwrap();
        assert_eq!(body["run"]["timed_out"], false);
        assert!(body["run"].get("error").is_none());

        let Json(full) = get_state(State(state.clone())).await.unwrap();
        assert_eq!(full.runs[&NodeId::new(2)], response.run);
        assert_eq!(full.progress.completed_questions, 0);
        // The block can run again once it has finished.
        assert!(
            post_run(&state, 2, Some("localhost:4000"), Some("abc"))
                .await
                .is_ok()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_second_run_of_a_running_block_conflicts() {
        let state = run_state(Some("abc"));
        // Admit a run the way the handler does, and leave it unfinished.
        state
            .session
            .lock()
            .unwrap()
            .begin_run(&state.lesson, NodeId::new(2), Some("abc"))
            .unwrap();
        let error = post_run(&state, 2, Some("127.0.0.1:4000"), Some("abc"))
            .await
            .unwrap_err();
        assert_eq!(
            (error.status, error.code),
            (StatusCode::CONFLICT, "run_in_progress")
        );
    }

    async fn body_text(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn embedded_frontend_serves_index_at_root() {
        let response = serve_asset("/".parse().unwrap()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html");
        assert!(
            body_text(response)
                .await
                .contains(r#"<div id="root"></div>"#)
        );
    }

    #[tokio::test]
    async fn unknown_paths_get_an_escaped_html_404_page() {
        for path in ["/assets/missing-abc123.js", "/some/client/route"] {
            let response = serve_asset(path.parse().unwrap()).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "text/html; charset=utf-8"
            );
            assert!(body_text(response).await.contains(path));
        }

        assert_eq!(
            escape_html(r#"<a href="x">&'"#),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
    }
}
