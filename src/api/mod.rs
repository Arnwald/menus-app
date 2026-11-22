//! Routeur HTTP et type d'erreur commun.

pub mod recipes;
pub mod settings;
pub mod sse;
pub mod weeks;

use crate::app::AppState;
use crate::weekid::WeekId;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use serde_json::json;

/// Erreur d'API rendue en JSON : `{"error": "..."}`.
#[derive(Debug)]
pub struct ApiError {
  pub status: StatusCode,
  pub message: String,
}

impl ApiError {
  pub fn bad_request(message: impl Into<String>) -> Self {
    ApiError {
      status: StatusCode::BAD_REQUEST,
      message: message.into(),
    }
  }
  pub fn not_found(message: impl Into<String>) -> Self {
    ApiError {
      status: StatusCode::NOT_FOUND,
      message: message.into(),
    }
  }
  pub fn conflict(message: impl Into<String>) -> Self {
    ApiError {
      status: StatusCode::CONFLICT,
      message: message.into(),
    }
  }
}

impl IntoResponse for ApiError {
  fn into_response(self) -> Response {
    (self.status, Json(json!({ "error": self.message }))).into_response()
  }
}

/// Toute erreur interne devient un 500 lisible.
impl From<anyhow::Error> for ApiError {
  fn from(err: anyhow::Error) -> Self {
    tracing::error!("erreur interne : {err:#}");
    ApiError {
      status: StatusCode::INTERNAL_SERVER_ERROR,
      message: format!("{err:#}"),
    }
  }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// Semaine prise dans l'URL : `2026-W47`, une date, `courante`, `prochaine`…
pub fn week_param(raw: &str) -> ApiResult<WeekId> {
  WeekId::parse_loose(raw).map_err(ApiError::bad_request)
}

pub fn router(state: AppState) -> Router {
  let routes = Router::new()
    .route("/state", get(settings::state))
    .route("/settings", get(settings::get_settings).patch(settings::patch_settings))
    .route("/ingredients", get(settings::get_ingredients))
    .route("/ingredients/{name}", put(settings::put_ingredient).delete(settings::delete_ingredient))
    .route("/recipes", get(recipes::list).post(recipes::create))
    .route(
      "/recipes/{id}",
      get(recipes::get_one).put(recipes::replace).patch(recipes::patch).delete(recipes::remove),
    )
    .route("/weeks", get(weeks::list_weeks))
    .route("/weeks/{week}", get(weeks::get_week))
    .route("/weeks/{week}/plan", delete(weeks::clear_plan))
    .route("/weeks/{week}/dinners/{day}", put(weeks::put_dinner))
    .route("/weeks/{week}/brunch/{day}", put(weeks::put_brunch))
    .route("/weeks/{week}/propose", post(weeks::propose))
    .route("/weeks/{week}/reroll/{day}", post(weeks::reroll))
    .route("/weeks/{week}/apply", post(weeks::apply))
    .route("/weeks/{week}/note", put(weeks::put_note))
    .route("/weeks/{week}/list", get(weeks::shopping_list))
    .route("/weeks/{week}/check", post(weeks::check))
    .route("/weeks/{week}/uncheck-all", post(weeks::uncheck_all))
    .route("/weeks/{week}/extras", post(weeks::add_extras))
    .route("/weeks/{week}/extras/{id}", patch(weeks::patch_extra).delete(weeks::delete_extra))
    .route("/stream", get(sse::stream))
    .route(
      "/chat",
      get(crate::chat::info)
        .post(crate::chat::send)
        // Une photo de recette passe dans la conversation.
        .layer(axum::extract::DefaultBodyLimit::max(16 << 20)),
    )
    .with_state(state.clone());

  Router::new().nest("/api", routes).layer(middleware::from_fn_with_state(state, auth))
}

/// Jeton facultatif : en-tête `Authorization: Bearer …`, `X-Auth-Token` ou `?token=`.
async fn auth(State(state): State<AppState>, request: Request, next: Next) -> Response {
  let Some(expected) = state.auth_token.as_deref() else {
    return next.run(request).await;
  };

  let headers = request.headers();
  let given = headers
    .get("x-auth-token")
    .and_then(|v| v.to_str().ok())
    .map(str::to_string)
    .or_else(|| {
      headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string)
    })
    .or_else(|| {
      request
        .uri()
        .query()
        .and_then(|q| q.split('&').find_map(|pair| pair.strip_prefix("token=").map(str::to_string)))
    });

  match given {
    Some(token) if token == expected => next.run(request).await,
    _ => (StatusCode::UNAUTHORIZED, Json(json!({ "error": "jeton d'accès invalide ou absent" }))).into_response(),
  }
}
