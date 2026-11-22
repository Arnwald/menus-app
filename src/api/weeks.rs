//! Semaines : soupers, brunchs, propositions, liste de courses, cases cochées.

use super::recipes::save_new;
use super::{week_param, ApiError, ApiResult};
use crate::app::AppState;
use crate::model::{parse_days, Day, Extra, Slot, Week};
use crate::planner::Planner;
use crate::recipes::RecipeDraft;
use crate::shopping;
use crate::store::Change;
use crate::weekid::WeekId;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn day_param(raw: &str) -> ApiResult<Day> {
  Day::parse(raw).ok_or_else(|| ApiError::bad_request(format!("jour inconnu : {raw}")))
}

/// Lit, modifie et réécrit une semaine sous le verrou d'écriture.
async fn update_week<T>(state: &AppState, id: WeekId, edit: impl FnOnce(&mut Week) -> ApiResult<T>) -> ApiResult<(Week, T)> {
  let _guard = state.write_lock.lock().await;
  let mut week = state.store.week(id)?;
  let out = edit(&mut week)?;
  state.store.save_week(&mut week)?;
  state.wrote(Change::week(id));
  Ok((week, out))
}

pub async fn list_weeks(State(state): State<AppState>) -> ApiResult<Json<Value>> {
  let mut out = vec![];
  for id in state.store.week_ids()? {
    let week = state.store.week(id)?;
    out.push(json!({ "week": id, "label": id.label(), "dinners": week.dinners.len(), "brunches": week.brunch.len() }));
  }
  Ok(Json(json!({ "weeks": out })))
}

pub async fn get_week(State(state): State<AppState>, Path(week): Path<String>) -> ApiResult<Json<Week>> {
  Ok(Json(state.store.week(week_param(&week)?)?))
}

/// Vide soupers et brunchs (garde la liste : cases cochées et ajouts).
pub async fn clear_plan(State(state): State<AppState>, Path(week): Path<String>) -> ApiResult<Json<Week>> {
  let (week, ()) = update_week(&state, week_param(&week)?, |w| {
    w.dinners.clear();
    w.brunch.clear();
    Ok(())
  })
  .await?;
  Ok(Json(week))
}

/// Un soir s'écrit `{"kind": "recipe", "recipe": "id"}`, `{"recipe": "id"}`,
/// ou simplement `"id"`, `"leftovers"`, `"free"`, `"empty"`.
#[derive(Deserialize)]
#[serde(untagged)]
pub enum SlotInput {
  Full(Slot),
  Short { recipe: String },
  Text(String),
}

impl SlotInput {
  fn into_slot(self) -> Slot {
    match self {
      SlotInput::Full(slot) => slot,
      SlotInput::Short { recipe } => Slot::recipe(recipe),
      SlotInput::Text(text) => match crate::slug::fold(&text).as_str() {
        "leftovers" | "restes" => Slot::Leftovers,
        "free" | "libre" => Slot::Free,
        "" | "empty" | "vide" | "none" => Slot::Empty,
        _ => Slot::recipe(text),
      },
    }
  }
}

pub async fn put_dinner(State(state): State<AppState>, Path((week, day)): Path<(String, String)>, Json(input): Json<SlotInput>) -> ApiResult<Json<Week>> {
  let (id, day) = (week_param(&week)?, day_param(&day)?);
  let slot = input.into_slot();
  if let Some(recipe) = slot.recipe_id() {
    ensure_recipes(&state, &[recipe])?;
  }
  let (week, ()) = update_week(&state, id, |w| {
    w.set_dinner(day, slot);
    Ok(())
  })
  .await?;
  Ok(Json(week))
}

#[derive(Deserialize)]
pub struct BrunchBody {
  recipes: Vec<String>,
}

pub async fn put_brunch(State(state): State<AppState>, Path((week, day)): Path<(String, String)>, Json(body): Json<BrunchBody>) -> ApiResult<Json<Week>> {
  let (id, day) = (week_param(&week)?, day_param(&day)?);
  let ids: Vec<&str> = body.recipes.iter().map(String::as_str).collect();
  ensure_recipes(&state, &ids)?;
  let (week, ()) = update_week(&state, id, |w| {
    w.set_brunch(day, body.recipes);
    Ok(())
  })
  .await?;
  Ok(Json(week))
}

fn ensure_recipes(state: &AppState, ids: &[&str]) -> ApiResult<()> {
  let recipes = state.store.recipes()?;
  for id in ids {
    if !recipes.iter().any(|r| r.id == *id) {
      return Err(ApiError::bad_request(format!("recette inconnue : {id}")));
    }
  }
  Ok(())
}

#[derive(Deserialize, Default)]
pub struct ProposeBody {
  /// Garde les soirs déjà remplis (défaut : oui).
  #[serde(default)]
  keep: Option<bool>,
  /// Graine du tirage, pour rejouer une proposition.
  #[serde(default)]
  seed: Option<u64>,
}

/// Proposition locale (sans Claude), écrite directement.
pub async fn propose(State(state): State<AppState>, Path(week): Path<String>, body: Option<Json<ProposeBody>>) -> ApiResult<Json<Week>> {
  let id = week_param(&week)?;
  let body = body.map(|Json(b)| b).unwrap_or_default();
  let recipes = state.store.recipes()?;
  let settings = state.store.settings()?;
  let history = state.store.history(id)?;
  let (week, ()) = update_week(&state, id, |w| {
    let planner = Planner {
      recipes: &recipes,
      settings: &settings,
      history: &history,
      month: id.month(),
    };
    let mut rng = StdRng::seed_from_u64(body.seed.unwrap_or_else(|| rand::thread_rng().gen()));
    let proposed = planner.propose(w, body.keep.unwrap_or(true), &mut rng);
    w.dinners = proposed.dinners;
    w.brunch = proposed.brunch;
    Ok(())
  })
  .await?;
  Ok(Json(week))
}

/// Remplace la recette d'un soir par une autre.
pub async fn reroll(State(state): State<AppState>, Path((week, day)): Path<(String, String)>) -> ApiResult<Json<Week>> {
  let (id, day) = (week_param(&week)?, day_param(&day)?);
  let recipes = state.store.recipes()?;
  let settings = state.store.settings()?;
  let history = state.store.history(id)?;
  let (week, ()) = update_week(&state, id, |w| {
    let planner = Planner {
      recipes: &recipes,
      settings: &settings,
      history: &history,
      month: id.month(),
    };
    let mut rng = StdRng::from_entropy();
    let other = planner
      .reroll(w, day, &mut rng)
      .ok_or_else(|| ApiError::conflict("aucune autre recette ne convient pour ce soir"))?;
    w.set_dinner(day, Slot::recipe(other));
    Ok(())
  })
  .await?;
  Ok(Json(week))
}

/// Applique une proposition (de Claude, typiquement) : nouvelles recettes
/// créées d'abord, puis leurs identifiants provisoires remplacés.
#[derive(Deserialize)]
pub struct ApplyBody {
  dinners: BTreeMap<Day, Slot>,
  #[serde(default)]
  brunch: BTreeMap<Day, Vec<String>>,
  #[serde(default)]
  new_recipes: Vec<RecipeDraft>,
  #[serde(default)]
  note: Option<String>,
}

pub async fn apply(State(state): State<AppState>, Path(week): Path<String>, Json(body): Json<ApplyBody>) -> ApiResult<Json<Value>> {
  let id = week_param(&week)?;
  let _guard = state.write_lock.lock().await;

  // Création des nouvelles recettes, une par une pour suivre leur identifiant.
  let mut renamed: BTreeMap<String, String> = BTreeMap::new();
  let mut warnings = vec![];
  for draft in &body.new_recipes {
    let temporary = draft.id.clone().unwrap_or_default();
    let (created, w) = save_new(&state, std::slice::from_ref(draft), "claude")?;
    warnings.extend(w);
    if let Some(recipe) = created.into_iter().next() {
      renamed.insert(temporary, recipe.id);
    }
  }
  let resolve = |rid: &str| renamed.get(rid).cloned().unwrap_or_else(|| rid.to_string());

  let known: Vec<String> = state.store.recipes()?.into_iter().map(|r| r.id).collect();
  let mut week = state.store.week(id)?;
  week.dinners.clear();
  for (day, slot) in body.dinners {
    let slot = match slot {
      Slot::Recipe { recipe } => {
        let recipe = resolve(&recipe);
        if !known.contains(&recipe) {
          warnings.push(format!("{} : recette inconnue « {recipe} »", day.label()));
          continue;
        }
        Slot::recipe(recipe)
      }
      other => other,
    };
    week.set_dinner(day, slot);
  }
  week.brunch.clear();
  for (day, ids) in body.brunch {
    let ids: Vec<String> = ids.iter().map(|r| resolve(r)).filter(|r| known.contains(r)).collect();
    week.set_brunch(day, ids);
  }
  if let Some(note) = body.note.filter(|n| !n.trim().is_empty()) {
    week.note = Some(note);
  }
  state.store.save_week(&mut week)?;
  state.wrote(Change::scope("all"));
  Ok(Json(json!({ "week": week, "created": renamed, "warnings": warnings })))
}

#[derive(Deserialize)]
pub struct NoteBody {
  note: Option<String>,
}

pub async fn put_note(State(state): State<AppState>, Path(week): Path<String>, Json(body): Json<NoteBody>) -> ApiResult<Json<Week>> {
  let (week, ()) = update_week(&state, week_param(&week)?, |w| {
    w.note = body.note.filter(|n| !n.trim().is_empty());
    Ok(())
  })
  .await?;
  Ok(Json(week))
}

#[derive(Deserialize)]
pub struct ListQuery {
  days: Option<String>,
  format: Option<String>,
}

/// Liste de courses en JSON, ou en texte avec `?format=text`.
pub async fn shopping_list(State(state): State<AppState>, Path(week): Path<String>, Query(q): Query<ListQuery>) -> ApiResult<Response> {
  let id = week_param(&week)?;
  let days = parse_days(q.days.as_deref()).map_err(ApiError::bad_request)?;
  let settings = state.store.settings()?;
  let list = shopping::build(&state.store.week(id)?, &state.store.recipes()?, &state.store.catalog()?, &settings, &days);
  if q.format.as_deref() == Some("text") {
    let text = shopping::to_text(&list, &settings);
    return Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response());
  }
  Ok(Json(list).into_response())
}

#[derive(Deserialize)]
pub struct CheckBody {
  key: String,
  done: bool,
}

pub async fn check(State(state): State<AppState>, Path(week): Path<String>, Json(body): Json<CheckBody>) -> ApiResult<Json<Value>> {
  let (_, ()) = update_week(&state, week_param(&week)?, |w| {
    if body.done {
      w.checked.insert(body.key.clone());
    } else {
      w.checked.remove(&body.key);
    }
    Ok(())
  })
  .await?;
  Ok(Json(json!({ "key": body.key, "done": body.done })))
}

/// Décoche tout (articles et ajouts) pour repartir faire les courses.
pub async fn uncheck_all(State(state): State<AppState>, Path(week): Path<String>) -> ApiResult<Json<Week>> {
  let (week, ()) = update_week(&state, week_param(&week)?, |w| {
    w.checked.clear();
    for extra in &mut w.extras {
      extra.done = false;
    }
    Ok(())
  })
  .await?;
  Ok(Json(week))
}

#[derive(Deserialize)]
pub struct ExtrasBody {
  /// Un article, ou plusieurs séparés par des virgules ou des retours à la ligne.
  text: String,
}

pub async fn add_extras(State(state): State<AppState>, Path(week): Path<String>, Json(body): Json<ExtrasBody>) -> ApiResult<Json<Vec<Extra>>> {
  let items: Vec<String> = body
    .text
    .split([',', '\n', ';'])
    .map(|t| t.trim().to_string())
    .filter(|t| !t.is_empty())
    .collect();
  if items.is_empty() {
    return Err(ApiError::bad_request("rien à ajouter"));
  }
  let (_, added) = update_week(&state, week_param(&week)?, |w| {
    let mut added = vec![];
    for text in items {
      let extra = Extra {
        id: format!("x{:08x}", rand::thread_rng().gen::<u32>()),
        text,
        done: false,
      };
      w.extras.push(extra.clone());
      added.push(extra);
    }
    Ok(added)
  })
  .await?;
  Ok(Json(added))
}

#[derive(Deserialize)]
pub struct ExtraPatch {
  done: Option<bool>,
  text: Option<String>,
}

pub async fn patch_extra(State(state): State<AppState>, Path((week, extra)): Path<(String, String)>, Json(patch): Json<ExtraPatch>) -> ApiResult<Json<Extra>> {
  let (_, updated) = update_week(&state, week_param(&week)?, |w| {
    let item = w
      .extras
      .iter_mut()
      .find(|e| e.id == extra)
      .ok_or_else(|| ApiError::not_found("article introuvable"))?;
    if let Some(done) = patch.done {
      item.done = done;
    }
    if let Some(text) = patch.text.filter(|t| !t.trim().is_empty()) {
      item.text = text.trim().to_string();
    }
    Ok(item.clone())
  })
  .await?;
  Ok(Json(updated))
}

pub async fn delete_extra(State(state): State<AppState>, Path((week, extra)): Path<(String, String)>) -> ApiResult<Json<Value>> {
  update_week(&state, week_param(&week)?, |w| {
    let before = w.extras.len();
    w.extras.retain(|e| e.id != extra);
    if w.extras.len() == before {
      return Err(ApiError::not_found("article introuvable"));
    }
    Ok(())
  })
  .await?;
  Ok(Json(json!({ "deleted": extra })))
}
