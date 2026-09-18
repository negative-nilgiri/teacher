use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde::Serialize;
use tokio::net::TcpListener;

use crate::source::NodeId;

use super::model::{
    ArtifactLoadError, QuestionMutationResponse, RuntimeLesson, StateResponse, SubmitRequest,
    load_artifact, project_runtime_lesson,
};
use super::session::{Session, SessionError};

#[derive(Clone)]
struct AppState {
    lesson: Arc<RuntimeLesson>,
    session: Arc<Mutex<Session>>,
}

impl AppState {
    fn new(lesson: RuntimeLesson) -> Self {
        let session = Session::new(&lesson);
        Self {
            lesson: Arc::new(lesson),
            session: Arc::new(Mutex::new(session)),
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
pub async fn bind(artifact_path: impl AsRef<Path>) -> Result<BoundServer, RuntimeError> {
    let artifact = load_artifact(artifact_path).map_err(RuntimeError::Artifact)?;
    let lesson = project_runtime_lesson(&artifact);
    let app = router(AppState::new(lesson));
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .map_err(RuntimeError::Bind)?;
    let address = listener.local_addr().map_err(RuntimeError::Bind)?;
    Ok(BoundServer {
        listener,
        app,
        address,
    })
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/state", get(get_state))
        .route("/api/v1/questions/{node_id}/submit", post(submit))
        .route("/api/v1/questions/{node_id}/reveal", post(reveal))
        .fallback(serve_asset)
        .with_state(state)
}

async fn get_state(State(state): State<AppState>) -> Result<Json<StateResponse>, ApiError> {
    let progress = state
        .session
        .lock()
        .map_err(|_| ApiError::internal())?
        .progress();
    Ok(Json(StateResponse {
        lesson: state.lesson.public.clone(),
        progress,
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

#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct WebAssets;

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
    let asset = WebAssets::get(asset_name)
        .map(|asset| (asset_name, asset))
        .or_else(|| WebAssets::get("index.html").map(|asset| ("index.html", asset)));
    match asset {
        Some((served_name, asset)) => {
            let content_type = mime_guess::from_path(served_name)
                .first_or_octet_stream()
                .as_ref()
                .to_owned();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(asset.data.into_owned()))
                .expect("static response is valid")
        }
        None => ApiError::not_found("asset_not_found", "Frontend asset not found").into_response(),
    }
}

#[derive(Debug)]
pub enum RuntimeError {
    Artifact(ArtifactLoadError),
    Bind(std::io::Error),
    Serve(std::io::Error),
}

impl RuntimeError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Artifact(error) => error.code(),
            Self::Bind(_) => "server_bind_failed",
            Self::Serve(_) => "server_failed",
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
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Artifact(error) => Some(error),
            Self::Bind(error) | Self::Serve(error) => Some(error),
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
    use crate::runtime::model::{project_runtime_lesson, tests::quiz_artifact};

    fn state() -> AppState {
        AppState::new(project_runtime_lesson(&quiz_artifact()))
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

    #[tokio::test]
    async fn embedded_frontend_has_an_index_and_spa_fallback() {
        assert!(WebAssets::get("index.html").is_some());
        let response = serve_asset("/some/client/route".parse().unwrap()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html");
    }
}
