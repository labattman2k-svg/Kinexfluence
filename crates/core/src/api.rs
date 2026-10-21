//! HTTP API for projects, snippets, and cart.
//!
//! The CLI talks to the domain modules directly; this module is for the Atlas UI and
//! any other client that would rather not link the crate. Every handler is a
//! thin call into the domain, so the rules live in one place and the HTTP layer
//! only decides status codes and JSON shapes.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::error;

use crate::cart::{self, CartError, CartItem};
use crate::embeddings::Embedder;
use crate::project::{self, CreateProject, Project, ProjectError};
use crate::scorer::{self, ScoreDetails, ScoredSnippetInput};
use crate::snippet::Snippet;

/// The body every error response carries.
#[derive(Debug, Serialize)]
pub struct ApiError {
    /// Machine-readable code: `not_found`, `invalid_input`, `conflict`, or
    /// `internal`. Clients branch on this, not on the message.
    pub error: &'static str,
    /// Human-readable explanation.
    pub message: String,
}

impl ApiError {
    fn new(error: &'static str, message: impl Into<String>) -> Self {
        Self {
            error,
            message: message.into(),
        }
    }

    fn status(&self) -> StatusCode {
        match self.error {
            "not_found" => StatusCode::NOT_FOUND,
            "invalid_input" => StatusCode::BAD_REQUEST,
            "conflict" => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        (status, Json(self)).into_response()
    }
}

impl From<ProjectError> for ApiError {
    fn from(error: ProjectError) -> Self {
        match error {
            ProjectError::NotFound(message) => Self::new("not_found", message),
            ProjectError::EmptyName => Self::new("invalid_input", "project name is required"),
            ProjectError::InvalidSlug(message) => Self::new("invalid_input", message),
            ProjectError::InvalidStatus(message) => Self::new("invalid_input", message),
            ProjectError::DuplicateSlug(message) => {
                Self::new("conflict", format!("duplicate slug: {message}"))
            }
            // The database message can carry schema and query detail, so it is
            // logged rather than returned. A client on the other end of a
            // socket gets a code it can act on and nothing it can mine.
            ProjectError::Db(source) => {
                error!(target: "kfx.api", error = %source, "database failure serving a project request");
                Self::new("internal", "internal error")
            }
        }
    }
}

impl From<CartError> for ApiError {
    fn from(error: CartError) -> Self {
        match error {
            CartError::SnippetNotFound(message) => Self::new("not_found", message),
            CartError::ProjectNotFound(message) => Self::new("not_found", message),
            CartError::Database(source) => {
                error!(target: "kfx.api", error = %source, "database failure serving a cart request");
                Self::new("internal", "internal error")
            }
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(source: sqlx::Error) -> Self {
        error!(target: "kfx.api", error = %source, "database failure");
        Self::new("internal", "internal error")
    }
}

/// A project as the API renders it.
///
/// Separate from [`Project`] on purpose, and not because of the field list:
/// `project.rs` is outside this change's scope, so `Project` does not derive
/// `Serialize`. Having an explicit wire type also means a change to the domain
/// struct cannot silently change the JSON a client depends on.
#[derive(Debug, Serialize)]
pub struct ProjectResponse {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub brief: Option<String>,
    pub status: String,
    pub weights_json: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Project> for ProjectResponse {
    fn from(project: Project) -> Self {
        Self {
            id: project.id,
            name: project.name,
            slug: project.slug,
            description: project.description,
            brief: project.brief,
            status: project.status,
            weights_json: project.weights_json,
            created_at: project.created_at,
            updated_at: project.updated_at,
        }
    }
}

/// The body `POST /v1/projects` accepts.
#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub brief: Option<String>,
}

/// The body `PATCH /v1/projects/:id_or_slug` accepts.
#[derive(Debug, Deserialize)]
pub struct UpdateProjectRequest {
    pub status: String,
}

/// The project routes, with the pool attached as router state.
///
/// The embedder rides along in a layer rather than in the router state: the
/// pool is the only thing every handler needs, and the embedder is needed by
/// exactly one. A layer keeps the state type of the router as the pool, so
/// the handlers that do not embed anything are unchanged.
pub fn router(pool: SqlitePool, embedder: Option<Arc<Embedder>>) -> Router {
    Router::new()
        .route("/v1/projects", get(list_projects).post(create_project))
        .route(
            "/v1/projects/:id_or_slug",
            get(get_project).patch(update_project),
        )
        .route(
            "/v1/projects/:id/snippets/scored",
            get(list_scored_snippets),
        )
        .route("/v1/snippets", get(list_snippets))
        .route("/v1/projects/:id/cart", get(list_cart).post(add_to_cart))
        .route("/v1/projects/:id/cart/:item_id", delete(remove_from_cart))
        .layer(Extension(embedder))
        .with_state(pool)
}

async fn list_projects(
    State(pool): State<SqlitePool>,
) -> Result<Json<Vec<ProjectResponse>>, ApiError> {
    let projects = project::list(&pool).await?;
    Ok(Json(projects.into_iter().map(Into::into).collect()))
}

async fn create_project(
    State(pool): State<SqlitePool>,
    Json(body): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<ProjectResponse>), ApiError> {
    let created = project::create(
        &pool,
        CreateProject {
            name: body.name,
            slug: body.slug,
            description: body.description,
            brief: body.brief,
        },
    )
    .await?;

    Ok((StatusCode::CREATED, Json(created.into())))
}

async fn get_project(
    State(pool): State<SqlitePool>,
    Path(id_or_slug): Path<String>,
) -> Result<Json<ProjectResponse>, ApiError> {
    Ok(Json(resolve(&pool, &id_or_slug).await?.into()))
}

async fn update_project(
    State(pool): State<SqlitePool>,
    Path(id_or_slug): Path<String>,
    Json(body): Json<UpdateProjectRequest>,
) -> Result<Json<ProjectResponse>, ApiError> {
    let project = resolve(&pool, &id_or_slug).await?;

    project::update_status(&pool, &project.id, &body.status).await?;

    // Reload rather than patching the in-memory copy: `updated_at` is set by
    // the database, so the only honest status field is the stored one.
    let updated = project::get(&pool, &project.id)
        .await?
        .ok_or_else(|| ApiError::new("not_found", format!("project not found: {id_or_slug}")))?;

    Ok(Json(updated.into()))
}

async fn resolve(pool: &SqlitePool, id_or_slug: &str) -> Result<Project, ApiError> {
    project::resolve(pool, id_or_slug)
        .await?
        .ok_or_else(|| ApiError::new("not_found", format!("project not found: {id_or_slug}")))
}

/// The body `GET /v1/snippets` accepts as query params.
#[derive(Debug, Deserialize)]
pub struct ListSnippetsQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

async fn list_snippets(
    State(pool): State<SqlitePool>,
    axum::extract::Query(query): axum::extract::Query<ListSnippetsQuery>,
) -> Result<Json<Vec<Snippet>>, ApiError> {
    let snippets = crate::snippet::list(&pool, Some(query.limit), Some(query.offset)).await?;
    Ok(Json(snippets))
}

/// The body `POST /v1/projects/:id/cart` accepts.
#[derive(Debug, Deserialize)]
pub struct AddToCartRequest {
    pub snippet_ids: Vec<String>,
}

async fn add_to_cart(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
    Json(body): Json<AddToCartRequest>,
) -> Result<(StatusCode, Json<Vec<String>>), ApiError> {
    let mut item_ids = Vec::new();
    for snippet_id in body.snippet_ids {
        let item_id = cart::add_to_cart(&pool, &id, &snippet_id).await?;
        item_ids.push(item_id);
    }
    Ok((StatusCode::CREATED, Json(item_ids)))
}

async fn list_cart(
    State(pool): State<SqlitePool>,
    Path(id): Path<String>,
) -> Result<Json<Vec<CartItem>>, ApiError> {
    let items = cart::list_cart(&pool, &id).await?;
    Ok(Json(items))
}

async fn remove_from_cart(
    State(pool): State<SqlitePool>,
    Path((project_id, item_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    cart::remove_from_cart(&pool, &project_id, &item_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A snippet scored against a project profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredSnippetResponse {
    pub id: String,
    pub message_id: String,
    pub block_index: i32,
    pub segment_index: i32,
    pub granularity: String,
    pub kind: String,
    pub text: String,
    pub language: Option<String>,
    pub char_start: Option<i32>,
    pub char_end: Option<i32>,
    pub hash: String,
    pub created_at: String,
    pub score: f32,
    pub score_details: ScoreDetails,
}

async fn list_scored_snippets(
    State(pool): State<SqlitePool>,
    Extension(embedder): Extension<Option<Arc<Embedder>>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ScoredSnippetResponse>>, ApiError> {
    let project = resolve(&pool, &id).await?;
    let profile = scorer::build_profile(&project, embedder.as_deref());

    let snippets = crate::snippet::list_with_metadata(&pool).await?;

    let mut scored: Vec<ScoredSnippetResponse> = snippets
        .into_iter()
        .map(|s| {
            let input = ScoredSnippetInput {
                kind: s.kind.clone(),
                text: s.text.clone(),
                role: s.role.clone(),
                turn_index: s.turn_index,
                total_turns: s.total_turns,
            };

            // The snippet's embedding is computed here, on the fly. A snippet
            // that cannot be embedded scores with a neutral S1 rather than
            // failing the whole request: one bad snippet must not cost the
            // user the entire list.
            let snippet_embedding = match embedder.as_deref() {
                Some(embedder) => match embedder.embed_text(&input.text) {
                    Ok(vector) => Some(vector),
                    Err(cause) => {
                        tracing::warn!(error = %cause, "could not embed a snippet");
                        None
                    }
                },
                None => None,
            };

            let score_details = scorer::score(&profile, &input, snippet_embedding.as_deref());
            ScoredSnippetResponse {
                id: s.id,
                message_id: s.message_id,
                block_index: s.block_index,
                segment_index: s.segment_index,
                granularity: s.granularity,
                kind: s.kind,
                text: s.text,
                language: s.language,
                char_start: s.char_start,
                char_end: s.char_end,
                hash: s.hash,
                created_at: s.created_at,
                score: score_details.score,
                score_details,
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });

    Ok(Json(scored))
}
