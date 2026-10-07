use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::body::{Body, Bytes};
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream;
use rust_embed::RustEmbed;
use serde::Serialize;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::TcpListener;

use crate::source::NodeId;

use super::media::{self, ByteRange, MediaFile};
use super::model::{
    ArtifactLoadError, PublicLessonNodeContent, QuestionMutationResponse, RunResponse,
    RuntimeLesson, StateResponse, SubmitRequest, load_artifact, project_runtime_lesson,
};
use super::runner::{self, RunResult};
use super::session::{Session, SessionError};

/// The header that carries the per-launch run token. A custom header makes a
/// cross-origin browser request need a CORS preflight, which this server never
/// answers.
const RUN_TOKEN_HEADER: &str = "x-learn-token";

/// How much of a media file is read at a time while it is sent.
const FILE_CHUNK: u64 = 64 * 1024;

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
    missing_media: Vec<MissingMedia>,
}

/// A media file a lesson refers to that was not in the sidecar directory when
/// the server started.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingMedia {
    /// The source ID of the block.
    pub block: String,
    pub file: String,
    /// The absolute sidecar directory that was searched.
    pub sidecar_dir: PathBuf,
}

impl BoundServer {
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// The media files that were absent at startup, for the launcher to warn
    /// about. A missing file is normal: the block shows its fallback text, and
    /// the file may still be produced while the server runs.
    pub fn missing_media(&self) -> &[MissingMedia] {
        &self.missing_media
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
/// The sidecar directory for media files is derived from `artifact_path` once,
/// here, as an absolute path.
///
/// With `allow_run`, the learner can run the lesson's `run_code` blocks, which
/// need the random token the state response then carries. Without it no run is
/// possible.
pub async fn bind(
    artifact_path: impl AsRef<Path>,
    allow_run: bool,
) -> Result<BoundServer, RuntimeError> {
    let artifact_path = artifact_path.as_ref();
    let display_path = artifact_path.to_string_lossy().into_owned();
    let artifact = load_artifact(artifact_path).map_err(RuntimeError::Artifact)?;
    let mut lesson = project_runtime_lesson(&artifact);
    lesson.public.artifact_path = Some(display_path);
    let sidecar_dir = media::sidecar_dir(artifact_path);
    let missing_media = missing_media(&lesson, &sidecar_dir);
    lesson.sidecar_dir = Some(sidecar_dir);
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
        missing_media,
    })
}

fn missing_media(lesson: &RuntimeLesson, sidecar_dir: &Path) -> Vec<MissingMedia> {
    lesson
        .public
        .nodes
        .iter()
        .filter_map(|node| match &node.content {
            PublicLessonNodeContent::ExternalArtifact { file, .. }
                if media::find(sidecar_dir, file).is_none() =>
            {
                Some(MissingMedia {
                    block: node.source_id.clone(),
                    file: file.clone(),
                    sidecar_dir: sidecar_dir.to_owned(),
                })
            }
            _ => None,
        })
        .collect()
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
        .route("/api/v1/artifacts/{node_id}/file", get(artifact_file))
        .fallback(serve_asset)
        .with_state(state)
}

async fn get_state(State(state): State<AppState>) -> Result<Json<StateResponse>, ApiError> {
    let session = state.session.lock().map_err(|_| ApiError::internal())?;
    Ok(Json(StateResponse {
        lesson: state.lesson.public_now(),
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

/// The media file of an `external_artifact` block, whole or one byte range of
/// it. Only the file name frozen in the artifact is ever looked up.
async fn artifact_file(
    State(state): State<AppState>,
    AxumPath(node_id): AxumPath<u32>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let node_id = NodeId::new(node_id);
    let file = external_file(&state.lesson, node_id)?;
    let missing = || {
        ApiError::not_found(
            "file_missing",
            format!("The file {file} of node {node_id} is not in the sidecar directory"),
        )
    };
    let MediaFile { path, len, .. } = state
        .lesson
        .sidecar_dir
        .as_deref()
        .and_then(|dir| media::find(dir, file))
        .ok_or_else(missing)?;

    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok());
    let (status, start, end) = match media::parse_range(range, len) {
        ByteRange::Whole => (StatusCode::OK, 0, len.saturating_sub(1)),
        ByteRange::Slice { start, end } => (StatusCode::PARTIAL_CONTENT, start, end),
        ByteRange::Unsatisfiable => {
            let mut response = ApiError {
                status: StatusCode::RANGE_NOT_SATISFIABLE,
                code: "range_not_satisfiable",
                message: format!("The file has {len} bytes"),
            }
            .into_response();
            response
                .headers_mut()
                .insert(header::CONTENT_RANGE, content_range(format!("*/{len}")));
            return Ok(response);
        }
    };
    let length = if len == 0 { 0 } else { end - start + 1 };
    let body = open_slice(&path, start, length)
        .await
        .map_err(|_| missing())?;

    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, media::content_type(file))
        .header(header::CONTENT_LENGTH, length)
        .header(header::ACCEPT_RANGES, "bytes")
        // The file may be replaced while `learn` runs; always ask again.
        .header(header::CACHE_CONTROL, "no-cache")
        // A file opened directly, such as an SVG, must not run script with the
        // authority of the lesson page.
        .header(header::CONTENT_SECURITY_POLICY, "sandbox")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    if status == StatusCode::PARTIAL_CONTENT {
        response = response.header(
            header::CONTENT_RANGE,
            content_range(format!("{start}-{end}/{len}")),
        );
    }
    Ok(response.body(body).expect("file response is valid"))
}

/// The file name of an `external_artifact` block.
fn external_file(lesson: &RuntimeLesson, node_id: NodeId) -> Result<&str, ApiError> {
    let node = lesson
        .public
        .nodes
        .iter()
        .find(|node| node.node_id == node_id);
    match node.map(|node| &node.content) {
        Some(PublicLessonNodeContent::ExternalArtifact { file, .. }) => Ok(file),
        Some(_) => Err(ApiError::not_found(
            "not_external_artifact",
            format!("Node {node_id} is not an external_artifact block"),
        )),
        None => Err(ApiError::not_found(
            "unknown_node",
            format!("Node {node_id} does not exist"),
        )),
    }
}

fn content_range(range: String) -> header::HeaderValue {
    header::HeaderValue::from_str(&format!("bytes {range}")).expect("a content range is ASCII")
}

/// Stream `length` bytes of the file from `start`, so a large video is never
/// held in memory.
async fn open_slice(path: &Path, start: u64, length: u64) -> std::io::Result<Body> {
    let mut file = File::open(path).await?;
    file.seek(std::io::SeekFrom::Start(start)).await?;
    let chunks = stream::unfold((file, length), |(mut file, remaining)| async move {
        if remaining == 0 {
            return None;
        }
        let mut chunk = vec![0; remaining.min(FILE_CHUNK) as usize];
        match file.read(&mut chunk).await {
            // The file shrank after it was measured; the body ends short.
            Ok(0) => None,
            Ok(read) => {
                chunk.truncate(read);
                Some((Ok(Bytes::from(chunk)), (file, remaining - read as u64)))
            }
            Err(error) => Some((Err(error), (file, 0))),
        }
    });
    Ok(Body::from_stream(chunks))
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
        tests::{media_artifact, quiz_artifact, run_artifact},
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

    /// A directory that is removed again when it goes out of scope.
    struct Sidecar(PathBuf);

    impl Sidecar {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "agent-teacher-server-{label}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn state(&self) -> AppState {
            let mut lesson = project_runtime_lesson(&media_artifact());
            lesson.sidecar_dir = Some(self.0.clone());
            AppState::new(lesson, None, PORT)
        }
    }

    impl Drop for Sidecar {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn get_file(
        state: &AppState,
        node: u32,
        range: Option<&str>,
    ) -> Result<Response, ApiError> {
        let mut headers = HeaderMap::new();
        if let Some(range) = range {
            headers.insert(header::RANGE, range.parse().unwrap());
        }
        artifact_file(State(state.clone()), AxumPath(node), headers).await
    }

    async fn body_bytes(response: Response) -> Vec<u8> {
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec()
    }

    fn header_of(response: &Response, name: header::HeaderName) -> &str {
        response.headers()[name].to_str().unwrap()
    }

    #[tokio::test]
    async fn the_file_of_a_media_block_is_served_whole_with_its_headers() {
        let sidecar = Sidecar::new("whole");
        let state = sidecar.state();
        std::fs::write(sidecar.0.join("demo.mp4"), b"0123456789").unwrap();

        let response = get_file(&state, 1, None).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(header_of(&response, header::CONTENT_TYPE), "video/mp4");
        assert_eq!(header_of(&response, header::CONTENT_LENGTH), "10");
        assert_eq!(header_of(&response, header::ACCEPT_RANGES), "bytes");
        assert_eq!(header_of(&response, header::CACHE_CONTROL), "no-cache");
        assert!(response.headers().get(header::CONTENT_RANGE).is_none());
        assert_eq!(body_bytes(response).await, b"0123456789");

        // A header that is not one valid range, and several ranges, give it all.
        for range in ["bytes=2-1", "bytes=0-1,4-5", "pages=1-2"] {
            let response = get_file(&state, 1, Some(range)).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{range}");
            assert_eq!(body_bytes(response).await, b"0123456789");
        }
    }

    #[tokio::test]
    async fn a_byte_range_gets_that_slice_and_its_content_range() {
        let sidecar = Sidecar::new("range");
        let state = sidecar.state();
        std::fs::write(sidecar.0.join("demo.mp4"), b"0123456789").unwrap();

        for (range, slice, content_range) in [
            ("bytes=2-4", "234", "bytes 2-4/10"),
            ("bytes=7-", "789", "bytes 7-9/10"),
            ("bytes=-3", "789", "bytes 7-9/10"),
            ("bytes=8-99", "89", "bytes 8-9/10"),
            ("bytes=0-0", "0", "bytes 0-0/10"),
        ] {
            let response = get_file(&state, 1, Some(range)).await.unwrap();
            assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT, "{range}");
            assert_eq!(header_of(&response, header::CONTENT_RANGE), content_range);
            assert_eq!(
                header_of(&response, header::CONTENT_LENGTH),
                slice.len().to_string()
            );
            assert_eq!(header_of(&response, header::ACCEPT_RANGES), "bytes");
            assert_eq!(body_bytes(response).await, slice.as_bytes(), "{range}");
        }

        for range in ["bytes=10-", "bytes=10-20", "bytes=-0"] {
            let response = get_file(&state, 1, Some(range)).await.unwrap();
            assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
            assert_eq!(header_of(&response, header::CONTENT_RANGE), "bytes */10");
            let body: serde_json::Value =
                serde_json::from_slice(&body_bytes(response).await).unwrap();
            assert_eq!(body["code"], "range_not_satisfiable");
        }
    }

    #[tokio::test]
    async fn a_file_larger_than_one_chunk_is_streamed_intact() {
        let sidecar = Sidecar::new("large");
        let state = sidecar.state();
        let bytes: Vec<u8> = (0..(FILE_CHUNK * 2 + 123))
            .map(|n| (n % 251) as u8)
            .collect();
        std::fs::write(sidecar.0.join("diagram.png"), &bytes).unwrap();

        let response = get_file(&state, 2, None).await.unwrap();
        assert_eq!(header_of(&response, header::CONTENT_TYPE), "image/png");
        assert_eq!(body_bytes(response).await, bytes);
        let response = get_file(&state, 2, Some("bytes=65000-131100"))
            .await
            .unwrap();
        assert_eq!(body_bytes(response).await, bytes[65000..=131100]);
    }

    #[tokio::test]
    async fn an_empty_file_is_served_empty_and_has_no_satisfiable_range() {
        let sidecar = Sidecar::new("empty");
        let state = sidecar.state();
        std::fs::write(sidecar.0.join("diagram.png"), b"").unwrap();

        let response = get_file(&state, 2, None).await.unwrap();
        assert_eq!(header_of(&response, header::CONTENT_LENGTH), "0");
        assert!(body_bytes(response).await.is_empty());
        let response = get_file(&state, 2, Some("bytes=0-")).await.unwrap();
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(header_of(&response, header::CONTENT_RANGE), "bytes */0");
    }

    #[tokio::test]
    async fn file_requests_for_other_nodes_and_absent_files_have_typed_errors() {
        let sidecar = Sidecar::new("errors");
        let state = sidecar.state();
        for (node, code) in [
            (0, "not_external_artifact"),
            (99, "unknown_node"),
            (1, "file_missing"),
            (2, "file_missing"),
        ] {
            let error = get_file(&state, node, None).await.unwrap_err();
            assert_eq!((error.status, error.code), (StatusCode::NOT_FOUND, code));
        }

        // Without a sidecar directory at all.
        let no_sidecar = AppState::new(project_runtime_lesson(&media_artifact()), None, PORT);
        let error = get_file(&no_sidecar, 1, None).await.unwrap_err();
        assert_eq!(error.code, "file_missing");

        // A directory with the file's name is not a file, even next to a range.
        std::fs::create_dir(sidecar.0.join("demo.mp4")).unwrap();
        let error = get_file(&state, 1, Some("bytes=0-1")).await.unwrap_err();
        assert_eq!(error.code, "file_missing");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_link_to_a_file_outside_the_directory_is_not_served() {
        let sidecar = Sidecar::new("link");
        let state = sidecar.state();
        let outside = sidecar.0.join("outside.png");
        std::fs::write(&outside, b"outside").unwrap();
        let inner = sidecar.0.join("inner.assets");
        std::fs::create_dir(&inner).unwrap();
        std::os::unix::fs::symlink(&outside, inner.join("diagram.png")).unwrap();
        let mut lesson = project_runtime_lesson(&media_artifact());
        lesson.sidecar_dir = Some(inner);
        let state = AppState {
            lesson: Arc::new(lesson),
            ..state
        };

        let error = get_file(&state, 2, None).await.unwrap_err();
        assert_eq!(error.code, "file_missing");
    }

    #[tokio::test]
    async fn state_says_per_request_which_files_are_available() {
        let sidecar = Sidecar::new("state");
        let state = sidecar.state();
        let media = |response: StateResponse| {
            let value = serde_json::to_value(response).unwrap();
            (
                value["lesson"]["nodes"][1].clone(),
                value["lesson"]["nodes"][2].clone(),
            )
        };

        let Json(before) = get_state(State(state.clone())).await.unwrap();
        let (video, image) = media(before);
        assert_eq!(
            (&video["available"], &image["available"]),
            (&false.into(), &false.into())
        );
        assert!(video.get("version").is_none());

        std::fs::write(sidecar.0.join("diagram.png"), b"png").unwrap();
        let Json(after) = get_state(State(state.clone())).await.unwrap();
        let (video, image) = media(after);
        assert_eq!(video["available"], false);
        assert_eq!(image["available"], true);
        assert!(image["version"].as_str().unwrap().starts_with("3-"));
    }

    #[tokio::test]
    async fn the_startup_check_lists_only_files_that_are_absent() {
        let sidecar = Sidecar::new("startup");
        std::fs::write(sidecar.0.join("demo.mp4"), b"video").unwrap();
        let lesson = project_runtime_lesson(&media_artifact());
        assert_eq!(
            missing_media(&lesson, &sidecar.0),
            [MissingMedia {
                block: "media-2".into(),
                file: "diagram.png".into(),
                sidecar_dir: sidecar.0.clone(),
            }]
        );
        assert_eq!(missing_media(&lesson, &sidecar.0.join("none")).len(), 2);
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
