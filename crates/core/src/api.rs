//! HTTP API for projects.
//!
//! The CLI talks to `project.rs` directly; this module is for the Atlas UI and
//! any other client that would rather not link the crate. Every handler is a
//! thin call into `project`, so the rules live in one place and the HTTP layer
//! only decides status codes and JSON shapes.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::error;

use crate::project::{self, CreateProject, Project, ProjectError};

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
pub fn router(pool: SqlitePool) -> Router {
    Router::new()
        .route("/v1/projects", get(list_projects).post(create_project))
        .route(
            "/v1/projects/:id_or_slug",
            get(get_project).patch(update_project),
        )
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
