//! État complet pour l'interface, réglages et catalogue d'ingrédients.

use super::{week_param, ApiError, ApiResult};
use crate::app::AppState;
use crate::model::{parse_days, Ingredient, Settings};
use crate::shopping;
use crate::store::Change;
use crate::weekid::WeekId;
use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct StateQuery {
  week: Option<String>,
  days: Option<String>,
}

/// Tout ce dont l'interface a besoin pour une semaine, en un appel.
pub async fn state(State(state): State<AppState>, Query(q): Query<StateQuery>) -> ApiResult<Json<Value>> {
  let id = match q.week.as_deref() {
    Some(raw) if !raw.is_empty() => week_param(raw)?,
    _ => WeekId::default_view(),
  };
  let days = parse_days(q.days.as_deref()).map_err(ApiError::bad_request)?;
  let store = &state.store;
  let settings = store.settings()?;
  let recipes = store.recipes()?;
  let catalog = store.catalog()?;
  let week = store.week(id)?;
  let list = shopping::build(&week, &recipes, &catalog, &settings, &days);
  let history = store.history(id)?;
  let today = chrono::Local::now().date_naive();

  Ok(Json(json!({
    "week": week,
    "label": id.label(),
    "monday": id.monday(),
    "prev": id.add(-1),
    "next": id.add(1),
    "current_week": WeekId::of(today),
    "today": today,
    "settings": settings,
    "portions": settings.portions(),
    "recipes": recipes,
    "history": history,
    "list": list,
  })))
}

pub async fn get_settings(State(state): State<AppState>) -> ApiResult<Json<Settings>> {
  Ok(Json(state.store.settings()?))
}

/// Modification partielle : seuls les champs fournis changent.
pub async fn patch_settings(State(state): State<AppState>, Json(patch): Json<Value>) -> ApiResult<Json<Settings>> {
  let _guard = state.write_lock.lock().await;
  let current = state.store.settings()?;
  let mut value = serde_json::to_value(&current).map_err(anyhow::Error::from)?;
  let (Value::Object(target), Value::Object(changes)) = (&mut value, patch) else {
    return Err(ApiError::bad_request("objet JSON attendu"));
  };
  for (key, change) in changes {
    if !target.contains_key(&key) {
      return Err(ApiError::bad_request(format!("réglage inconnu : {key}")));
    }
    target.insert(key, change);
  }
  let settings: Settings = serde_json::from_value(value).map_err(|e| ApiError::bad_request(e.to_string()))?;
  settings.validate().map_err(ApiError::bad_request)?;
  state.store.save_settings(&settings)?;
  state.wrote(Change::scope("settings"));
  Ok(Json(settings))
}

pub async fn get_ingredients(State(state): State<AppState>) -> ApiResult<Json<Value>> {
  let catalog = state.store.catalog()?;
  let recipes = state.store.recipes()?;
  // Nombre de recettes qui emploient chaque ingrédient, pour le ménage.
  let items: Vec<Value> = catalog
    .items
    .iter()
    .map(|(name, ing)| {
      let used = recipes.iter().filter(|r| r.ingredients.iter().any(|l| &l.name == name)).count();
      json!({ "name": name, "aisle": ing.aisle, "unit": ing.unit, "pantry": ing.pantry, "used_by": used })
    })
    .collect();
  Ok(Json(json!({ "items": items })))
}

/// Crée ou remplace un ingrédient du catalogue.
pub async fn put_ingredient(State(state): State<AppState>, Path(name): Path<String>, Json(ingredient): Json<Ingredient>) -> ApiResult<Json<Ingredient>> {
  let _guard = state.write_lock.lock().await;
  let settings = state.store.settings()?;
  if !settings.has_aisle(&ingredient.aisle) {
    return Err(ApiError::bad_request(format!("rayon inconnu : {}", ingredient.aisle)));
  }
  let mut catalog = state.store.catalog()?;
  let key = catalog.resolve(&name).map(|(k, _)| k.to_string()).unwrap_or_else(|| name.trim().to_string());
  catalog.items.insert(key, ingredient.clone());
  state.store.save_catalog(&catalog)?;
  state.wrote(Change::scope("ingredients"));
  Ok(Json(ingredient))
}

#[derive(Deserialize)]
pub struct ForceQuery {
  #[serde(default)]
  force: bool,
}

pub async fn delete_ingredient(State(state): State<AppState>, Path(name): Path<String>, Query(q): Query<ForceQuery>) -> ApiResult<Json<Value>> {
  let _guard = state.write_lock.lock().await;
  let mut catalog = state.store.catalog()?;
  let key = catalog
    .resolve(&name)
    .map(|(k, _)| k.to_string())
    .ok_or_else(|| ApiError::not_found(format!("ingrédient inconnu : {name}")))?;
  let users: Vec<String> = state
    .store
    .recipes()?
    .into_iter()
    .filter(|r| r.ingredients.iter().any(|l| l.name == key))
    .map(|r| r.name)
    .collect();
  if !users.is_empty() && !q.force {
    return Err(ApiError::conflict(format!(
      "{key} sert encore dans : {} (ajoute ?force=true pour supprimer quand même)",
      users.join(", ")
    )));
  }
  catalog.items.remove(&key);
  state.store.save_catalog(&catalog)?;
  state.wrote(Change::scope("ingredients"));
  Ok(Json(json!({ "deleted": key })))
}
