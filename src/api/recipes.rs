//! Recettes : lecture, création (une ou plusieurs), remplacement, drapeaux, suppression.

use super::{ApiError, ApiResult};
use crate::app::AppState;
use crate::model::{Catalog, Recipe};
use crate::recipes::{check, Checked, RecipeDraft};
use crate::shopping::format_cooking;
use crate::store::Change;
use crate::weekid::WeekId;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<Recipe>>> {
  Ok(Json(state.store.recipes()?))
}

/// Une recette, avec les quantités calculées pour le foyer.
pub async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
  let recipes = state.store.recipes()?;
  let recipe = recipes
    .into_iter()
    .find(|r| r.id == id)
    .ok_or_else(|| ApiError::not_found(format!("recette inconnue : {id}")))?;
  let settings = state.store.settings()?;
  let catalog = state.store.catalog()?;
  let portions = settings.portions();
  let lines: Vec<Value> = recipe
    .ingredients
    .iter()
    .map(|l| {
      let pantry = catalog.resolve(&l.name).is_some_and(|(_, i)| i.pantry);
      json!({ "name": l.name, "amount": format_cooking(l.qty * portions, l.unit), "pantry": pantry })
    })
    .collect();
  Ok(Json(json!({ "recipe": recipe, "portions": portions, "lines": lines })))
}

/// Corps accepté par `POST /api/recipes` : une recette, une liste, ou
/// `{"recipes": [...], "source": "claude"}`.
#[derive(Deserialize)]
#[serde(untagged)]
pub enum CreateBody {
  Wrapped {
    recipes: Vec<RecipeDraft>,
    #[serde(default)]
    source: Option<String>,
  },
  Many(Vec<RecipeDraft>),
  One(Box<RecipeDraft>),
}

pub async fn create(State(state): State<AppState>, Json(body): Json<CreateBody>) -> ApiResult<(StatusCode, Json<Value>)> {
  let (drafts, source) = match body {
    CreateBody::Wrapped { recipes, source } => (recipes, source),
    CreateBody::Many(list) => (list, None),
    CreateBody::One(one) => (vec![*one], None),
  };
  if drafts.is_empty() {
    return Err(ApiError::bad_request("aucune recette fournie"));
  }
  let source = source.filter(|s| s == "claude").unwrap_or_else(|| "manual".into());

  let _guard = state.write_lock.lock().await;
  let (created, warnings) = save_new(&state, &drafts, &source)?;
  Ok((StatusCode::CREATED, Json(json!({ "created": created, "warnings": warnings }))))
}

/// Valide et enregistre de nouvelles recettes. Appelant : verrou d'écriture tenu.
pub fn save_new(state: &AppState, drafts: &[RecipeDraft], source: &str) -> ApiResult<(Vec<Recipe>, Vec<String>)> {
  let settings = state.store.settings()?;
  let mut catalog = state.store.catalog()?;
  let mut recipes = state.store.recipes()?;
  let mut created = vec![];
  let mut warnings = vec![];
  for draft in drafts {
    let checked = check(draft, &catalog, &settings, |id| recipes.iter().any(|r| r.id == id), None, source).map_err(ApiError::bad_request)?;
    absorb(&mut catalog, &checked);
    warnings.extend(checked.warnings);
    recipes.push(checked.recipe.clone());
    created.push(checked.recipe);
  }
  state.store.save_catalog(&catalog)?;
  state.store.save_recipes(&mut recipes)?;
  state.wrote(Change::scope("recipes"));
  Ok((created, warnings))
}

/// Ajoute au catalogue les ingrédients nouveaux d'une recette validée.
pub fn absorb(catalog: &mut Catalog, checked: &Checked) {
  for new in &checked.new_ingredients {
    catalog.items.entry(new.name.clone()).or_insert_with(|| new.ingredient.clone());
  }
}

/// Remplace entièrement une recette ; favori, exclusion et date de création
/// sont conservés s'ils ne sont pas fournis.
pub async fn replace(State(state): State<AppState>, Path(id): Path<String>, Json(draft): Json<RecipeDraft>) -> ApiResult<Json<Value>> {
  let _guard = state.write_lock.lock().await;
  let settings = state.store.settings()?;
  let mut catalog = state.store.catalog()?;
  let mut recipes = state.store.recipes()?;
  let index = recipes
    .iter()
    .position(|r| r.id == id)
    .ok_or_else(|| ApiError::not_found(format!("recette inconnue : {id}")))?;
  let old = recipes[index].clone();

  let mut draft = draft;
  draft.favorite = draft.favorite.or(Some(old.favorite));
  draft.excluded = draft.excluded.or(Some(old.excluded));
  let checked = check(&draft, &catalog, &settings, |_| false, Some(&id), &old.source).map_err(ApiError::bad_request)?;
  absorb(&mut catalog, &checked);
  let mut recipe = checked.recipe;
  recipe.created_at = old.created_at;
  recipes[index] = recipe.clone();

  state.store.save_catalog(&catalog)?;
  state.store.save_recipes(&mut recipes)?;
  state.wrote(Change::scope("recipes"));
  Ok(Json(json!({ "recipe": recipe, "warnings": checked.warnings })))
}

#[derive(Deserialize)]
pub struct RecipePatch {
  favorite: Option<bool>,
  excluded: Option<bool>,
  name: Option<String>,
  notes: Option<String>,
  minutes: Option<u32>,
  keeps: Option<bool>,
}

/// Petites retouches sans revalider les ingrédients (favori, exclusion…).
pub async fn patch(State(state): State<AppState>, Path(id): Path<String>, Json(patch): Json<RecipePatch>) -> ApiResult<Json<Recipe>> {
  let _guard = state.write_lock.lock().await;
  let mut recipes = state.store.recipes()?;
  let recipe = recipes
    .iter_mut()
    .find(|r| r.id == id)
    .ok_or_else(|| ApiError::not_found(format!("recette inconnue : {id}")))?;
  if let Some(v) = patch.favorite {
    recipe.favorite = v;
  }
  if let Some(v) = patch.excluded {
    recipe.excluded = v;
  }
  if let Some(v) = patch.name.filter(|n| !n.trim().is_empty()) {
    recipe.name = v.trim().to_string();
  }
  if let Some(v) = patch.notes {
    recipe.notes = Some(v).filter(|n| !n.trim().is_empty());
  }
  if let Some(v) = patch.minutes.filter(|m| *m > 0) {
    recipe.minutes = v;
    recipe.active_minutes = recipe.active_minutes.min(v);
  }
  if let Some(v) = patch.keeps {
    recipe.keeps = v;
  }
  recipe.updated_at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
  let updated = recipe.clone();
  state.store.save_recipes(&mut recipes)?;
  state.wrote(Change::scope("recipes"));
  Ok(Json(updated))
}

/// Supprime une recette et la retire des semaines à venir (les semaines
/// passées gardent leur historique).
pub async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
  let _guard = state.write_lock.lock().await;
  let mut recipes = state.store.recipes()?;
  let before = recipes.len();
  recipes.retain(|r| r.id != id);
  if recipes.len() == before {
    return Err(ApiError::not_found(format!("recette inconnue : {id}")));
  }
  state.store.save_recipes(&mut recipes)?;

  let current = WeekId::current();
  let mut touched = vec![];
  for week_id in state.store.week_ids()?.into_iter().filter(|w| *w >= current) {
    let mut week = state.store.week(week_id)?;
    if week.recipe_ids().any(|r| r == id) {
      week.replace_recipe(&id, None);
      state.store.save_week(&mut week)?;
      touched.push(week_id.to_string());
    }
  }
  state.wrote(Change::scope("all"));
  Ok(Json(json!({ "deleted": id, "weeks_updated": touched })))
}
