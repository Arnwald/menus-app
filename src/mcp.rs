//! `menus mcp` : le serveur MCP qu'un assistant (Claude Desktop, Claude Code) lance en stdio.
//! Il décrit les outils et relaie chaque appel à une instance `menus serve` par son API
//! HTTP, locale ou sur le NAS : la base reste au même endroit, et ce que l'assistant
//! change apparaît aussitôt sur les téléphones. Rien d'autre que le protocole ne passe
//! sur stdout ; les messages vont sur stderr.

use crate::model::{Day, Meal, Recipe, Settings, Slot, Week};
use crate::shopping::number;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::Duration;

pub(crate) const INSTRUCTIONS: &str = "menus plans a family's dinners (soupers) and weekend brunches week by \
week, and builds the matching Coop/Migros shopping list. The family lives in Valais, Switzerland, \
speaks French (souper, demi-crème, dl) and wants to eat better without spending its evenings \
cooking. These tools read and change the family's shared instance: every change shows at once \
on their phones.

How to help:
- Start with get_week (the plan, the household, which evenings are long, leftovers or free) and \
list_recipes (ids, durations, categories, when each was last served). Weeks are written \
2026-W48, as a date, or « courante » / « prochaine ».
- Balance of a week's dinners: 2 fish, at least 3 vegetarian evenings including 1 with legumes, \
at most 2 red meat and 2 poultry, starches that vary from one evening to the next. Ordinary \
evenings get dishes of at most quick_minutes; long evenings can take more. The evening before a \
« leftovers » evening needs a dish that keeps (keeps = true): it is cooked twice as much. Avoid \
dishes served in the last avoid_weeks weeks, prefer favourites (♥), respect the season.
- Prefer the family's recipes. Create new ones when the user asks, to use up an ingredient, or \
to bring variety; say which ones are new.
- plan_week replaces all dinners and brunches of the week: if the week already has a plan, show \
your proposal and get a yes first. For one evening, use set_dinner, set_brunch or reroll_dinner. \
propose_week draws a quick plan without you, as a starting point.
- Recipes: quantities for ONE adult (the app scales them to the household). Use the exact \
ingredient names and units of list_ingredients so the shopping list adds them up; a new \
ingredient needs its aisle. Never list salt, pepper or water. 3 to 6 short steps without \
quantities. A recipe pasted or photographed by the user: transcribe it faithfully, show it, then \
add_recipes. Read the warnings the tools return and mention the ones that matter.
- Delete a recipe or empty a week only when the user asks.";

// -- relais HTTP ----------------------------------------------------------------

#[derive(Clone)]
struct Api {
  http: reqwest::Client,
  base: String,
  token: Option<String>,
}

impl Api {
  async fn send(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<String, String> {
    let url = format!("{}{path}", self.base);
    let mut request = self.http.request(method, &url);
    if let Some(token) = &self.token {
      request = request.header("X-Auth-Token", token);
    }
    if let Some(body) = body {
      request = request.json(&body);
    }
    let response = request.send().await.map_err(|e| {
      // La cause réelle (refus, délai, réseau inaccessible) est dans la chaîne d'erreurs.
      let mut cause = e.to_string();
      let mut source = std::error::Error::source(&e);
      while let Some(s) = source {
        cause.push_str(" : ");
        cause.push_str(&s.to_string());
        source = s.source();
      }
      format!(
        "menus ne répond pas à {} ({cause}). Lancer `menus serve`, ou vérifier l'adresse donnée à `menus mcp --url`.",
        self.base
      )
    })?;
    let status = response.status();
    let text = response.text().await.map_err(|e| format!("réponse illisible : {e}"))?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
      return Err("l'instance exige un jeton : le donner à `menus mcp --token` (ou MENUS_TOKEN)".into());
    }
    if !status.is_success() {
      let message = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
        .unwrap_or(text);
      return Err(message);
    }
    Ok(text)
  }

  async fn get(&self, path: &str) -> Result<String, String> {
    self.send(reqwest::Method::GET, path, None).await
  }

  async fn json<T: serde::de::DeserializeOwned>(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<T, String> {
    let text = self.send(method, path, body).await?;
    serde_json::from_str(&text).map_err(|e| format!("réponse inattendue de menus : {e}"))
  }

  async fn state(&self, week: &str) -> Result<StateView, String> {
    self.json(reqwest::Method::GET, &format!("/api/state?week={}", enc(week)), None).await
  }
}

fn enc(s: &str) -> String {
  s.bytes()
    .map(|b| match b {
      b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
      _ => format!("%{b:02X}"),
    })
    .collect()
}

/// Ce que l'outil lit de `/api/state`.
#[derive(Deserialize)]
struct StateView {
  week: Week,
  label: String,
  settings: Settings,
  portions: f64,
  recipes: Vec<Recipe>,
  history: HashMap<String, u32>,
  list: ListView,
}

#[derive(Deserialize)]
struct ListView {
  total: usize,
  checked: usize,
}

fn ok(text: impl Into<String>) -> CallToolResult {
  CallToolResult::success(vec![ContentBlock::text(text)])
}

fn refusal(text: impl Into<String>) -> CallToolResult {
  CallToolResult::error(vec![ContentBlock::text(text)])
}

fn done(result: Result<String, String>) -> CallToolResult {
  match result {
    Ok(text) => ok(text),
    Err(why) => refusal(why),
  }
}

// -- paramètres -------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct WeekParam {
  #[schemars(
    description = "Week: 2026-W48, a date (2026-11-23), « courante » or « prochaine ». Default: the week the app shows (the next one from Sunday noon)"
  )]
  week: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListRecipesParams {
  #[schemars(description = "\"dinner\" or \"brunch\" (default: both)")]
  meal: Option<String>,
  #[schemars(description = "Words to find in the name or the ingredients")]
  query: Option<String>,
  #[schemars(description = "Only favourites")]
  favorites_only: Option<bool>,
  #[schemars(description = "Also list the recipes excluded from proposals (default: no)")]
  include_excluded: Option<bool>,
  #[schemars(description = "Week from which « last served » is counted (default: the week the app shows)")]
  week: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct IdParam {
  #[schemars(description = "Recipe id, as list_recipes gives it")]
  id: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum MealIn {
  Dinner,
  Brunch,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum BrunchKindIn {
  Sweet,
  Savory,
  Base,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum ProteinIn {
  Fish,
  Poultry,
  Meat,
  Veggie,
  Legumes,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct IngredientIn {
  #[schemars(description = "Name, exactly as in list_ingredients when it is there")]
  name: String,
  #[schemars(description = "Quantity for ONE adult")]
  qty: f64,
  #[schemars(description = "g, ml, pce, gousse, botte, tranche, cs, cc or pincée (kg, dl, l are converted)")]
  unit: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "Aisle id, for an ingredient not in list_ingredients (fl, bo, vi, po, ch, la, fr, pa, co, in, ep, pd, su, bs, di)")]
  aisle: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct RecipeIn {
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "Only in plan_week: a provisional id (new-1, new-2…) to use in dinners or brunch")]
  id: Option<String>,
  #[schemars(description = "Name in French, as the family would say it")]
  name: String,
  meal: MealIn,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "Brunch only: sweet, savory, or base (bread, butter, jam, juice)")]
  brunch_kind: Option<BrunchKindIn>,
  #[schemars(description = "Total time in minutes, waiting included")]
  minutes: u32,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "Minutes of actual work (default: minutes)")]
  active_minutes: Option<u32>,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "Dinners: main protein category")]
  protein: Option<ProteinIn>,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "rice, pasta, potato, bread, couscous, quinoa, polenta, noodles, tortilla, gnocchi or none")]
  starch: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "Reheats well the next day (can feed a « leftovers » evening)")]
  keeps: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "Months when the dish is in season, 1 to 12; empty or absent = all year")]
  months: Option<Vec<u32>>,
  ingredients: Vec<IngredientIn>,
  #[schemars(description = "3 to 6 short steps, without quantities")]
  steps: Vec<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  notes: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddRecipesParams {
  recipes: Vec<RecipeIn>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UpdateRecipeParams {
  #[schemars(description = "Id of the recipe to replace")]
  id: String,
  #[schemars(description = "The whole recipe as it should be (favourite and exclusion are kept)")]
  recipe: RecipeIn,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FlagsParams {
  id: String,
  #[schemars(description = "Favourite: proposed more often")]
  favorite: Option<bool>,
  #[schemars(description = "Excluded: never proposed (the family does not like it)")]
  excluded: Option<bool>,
  #[schemars(description = "A note shown on the recipe (empty string removes it)")]
  notes: Option<String>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct DinnersIn {
  mon: Option<String>,
  tue: Option<String>,
  wed: Option<String>,
  thu: Option<String>,
  fri: Option<String>,
  sat: Option<String>,
  sun: Option<String>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct BrunchIn {
  mon: Option<Vec<String>>,
  tue: Option<Vec<String>>,
  wed: Option<Vec<String>>,
  thu: Option<Vec<String>>,
  fri: Option<Vec<String>>,
  sat: Option<Vec<String>>,
  sun: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PlanWeekParams {
  #[schemars(description = "Week: 2026-W48, a date, « courante » or « prochaine »")]
  week: String,
  #[schemars(
    description = "Each evening: a recipe id, a provisional id from new_recipes, \"leftovers\", \"free\" or \"empty\". An evening left out is empty."
  )]
  dinners: DinnersIn,
  #[schemars(description = "Brunch days: list of recipe ids, usually base-brunch then one sweet and one savory")]
  brunch: Option<BrunchIn>,
  #[schemars(description = "Recipes to create first, each with a provisional id (new-1…)")]
  new_recipes: Option<Vec<RecipeIn>>,
  #[schemars(description = "One or two sentences for the family, shown above the week")]
  note: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DinnerParams {
  week: String,
  #[schemars(description = "mon…sun (or lun…dim)")]
  day: String,
  #[schemars(description = "A recipe id, \"leftovers\", \"free\" or \"empty\"")]
  value: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BrunchParams {
  week: String,
  #[schemars(description = "mon…sun (or lun…dim)")]
  day: String,
  #[schemars(description = "Recipe ids of the brunch, in order; empty list removes the brunch")]
  recipes: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProposeParams {
  week: String,
  #[schemars(description = "Keep the evenings already planned and fill only the empty ones (default: true)")]
  keep: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DayParams {
  week: String,
  #[schemars(description = "mon…sun (or lun…dim)")]
  day: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListParams {
  week: Option<String>,
  #[schemars(description = "Only the meals of these days, e.g. \"lun,mar,mer\" (default: the whole week)")]
  days: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ExtrasParams {
  week: String,
  #[schemars(description = "Items to add by hand, separated by commas: « lait, papier toilette »")]
  items: String,
}

#[derive(Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
struct SettingsIn {
  #[serde(skip_serializing_if = "Option::is_none")]
  adults: Option<u32>,
  #[serde(skip_serializing_if = "Option::is_none")]
  children: Option<u32>,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "Share of an adult portion a child eats, e.g. 0.6")]
  child_factor: Option<f64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "Evenings with time to cook, e.g. [\"sun\"]")]
  long_days: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "Evenings eating the previous day's leftovers, e.g. [\"thu\"]")]
  leftover_days: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "Evenings without a planned dinner")]
  free_days: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  brunch_days: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "Always add the base (bread, butter, jam, juice) to a brunch")]
  brunch_base: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "Longest dish for an ordinary evening, minutes")]
  quick_minutes: Option<u32>,
  #[serde(skip_serializing_if = "Option::is_none")]
  #[schemars(description = "Weeks before a dish is proposed again")]
  avoid_weeks: Option<u32>,
  #[serde(skip_serializing_if = "Option::is_none")]
  store_name: Option<String>,
}

// -- mise en forme ------------------------------------------------------------------

fn category(r: &Recipe) -> String {
  match r.meal {
    Meal::Dinner => r.protein.map(|p| p.label().to_string()).unwrap_or_default(),
    Meal::Brunch => match r.brunch_kind {
      Some(crate::model::BrunchKind::Sweet) => "brunch sucré".into(),
      Some(crate::model::BrunchKind::Savory) => "brunch salé".into(),
      _ => "brunch base".into(),
    },
  }
}

fn last_served(history: &HashMap<String, u32>, id: &str) -> String {
  match history.get(id) {
    Some(1) => "servi à 1 semaine".into(),
    Some(n) => format!("servi à {n} semaines"),
    None => "jamais servi (récemment)".into(),
  }
}

fn days(list: &[Day]) -> String {
  if list.is_empty() {
    "aucun".into()
  } else {
    list.iter().map(|d| d.label()).collect::<Vec<_>>().join(", ")
  }
}

/// Résumé d'une semaine, lisible par l'assistant comme par la famille.
fn week_summary(s: &StateView) -> String {
  let name = |id: &str| -> String {
    match s.recipes.iter().find(|r| r.id == id) {
      Some(r) => format!(
        "{} [{}] · {} min · {}{}",
        r.name,
        r.id,
        r.minutes,
        category(r),
        if r.keeps { " · se garde" } else { "" }
      ),
      None => format!("recette introuvable [{id}]"),
    }
  };
  let plan = crate::shopping::cooking_plan(&s.week);
  let times = |day: Day| plan.iter().find(|c| c.day == day && !c.brunch).map(|c| c.times).unwrap_or(1.0);
  let set = &s.settings;

  let mut out = String::new();
  let _ = writeln!(out, "Semaine {} ({})", s.week.week, s.label);
  let _ = writeln!(
    out,
    "Foyer : {} adulte(s), {} enfant(s) ({} portions). Soirs longs : {}. Restes : {}. Libres : {}. Brunchs : {}. Plat rapide : {} min au plus. Pas de répétition avant {} semaines.\n",
    set.adults,
    set.children,
    number(s.portions),
    days(&set.long_days),
    days(&set.leftover_days),
    days(&set.free_days),
    days(&set.brunch_days),
    set.quick_minutes,
    set.avoid_weeks
  );

  let mut last: Option<String> = None;
  for day in Day::ALL {
    if let Some(items) = s.week.brunch.get(&day) {
      let names: Vec<String> = items
        .iter()
        .map(|id| {
          s.recipes
            .iter()
            .find(|r| &r.id == id)
            .map(|r| format!("{} [{id}]", r.name))
            .unwrap_or(format!("[{id}]"))
        })
        .collect();
      let _ = writeln!(out, "{:<9} brunch : {}", day.label(), names.join(" + "));
    }
    let dinner = match s.week.dinner(day) {
      Slot::Recipe { recipe } => {
        last = Some(recipe.clone());
        let n = times(day);
        format!(
          "{}{}",
          name(recipe),
          if n > 1.0 { format!(" · préparer ×{}", number(n)) } else { String::new() }
        )
      }
      Slot::Leftovers => format!(
        "restes ({})",
        last
          .as_deref()
          .map(|id| s.recipes.iter().find(|r| r.id == id).map(|r| r.name.clone()).unwrap_or(id.to_string()))
          .unwrap_or("rien de cuisiné avant".into())
      ),
      Slot::Free => "libre".into(),
      Slot::Empty => "(vide)".into(),
    };
    let _ = writeln!(out, "{:<9} souper : {dinner}", day.label());
  }

  let dinners: Vec<&Recipe> = s
    .week
    .dinners
    .values()
    .filter_map(Slot::recipe_id)
    .filter_map(|id| s.recipes.iter().find(|r| r.id == id))
    .collect();
  let count = |p: crate::model::Protein| dinners.iter().filter(|r| r.protein == Some(p)).count();
  use crate::model::Protein::*;
  let _ = writeln!(
    out,
    "\nÉquilibre des soupers : {} poisson, {} végétarien(s) dont {} légumineuses, {} volaille, {} viande.",
    count(Fish),
    count(Veggie) + count(Legumes),
    count(Legumes),
    count(Poultry),
    count(Meat)
  );
  let _ = writeln!(out, "Liste de courses : {} article(s) coché(s) sur {}.", s.list.checked, s.list.total);
  if let Some(note) = &s.week.note {
    let _ = writeln!(out, "Note : {note}");
  }
  out
}

/// Transforme les soirs donnés par l'assistant en emplacements pour `/apply`.
fn dinner_slot(value: &str) -> Option<Value> {
  let slot = match crate::slug::fold(value).as_str() {
    "" | "empty" | "vide" | "none" => return None,
    "leftovers" | "restes" => json!({ "kind": "leftovers" }),
    "free" | "libre" => json!({ "kind": "free" }),
    _ => json!({ "kind": "recipe", "recipe": value.trim() }),
  };
  Some(slot)
}

fn warnings_text(value: &Value) -> String {
  let list: Vec<&str> = value
    .get("warnings")
    .and_then(Value::as_array)
    .map(|w| w.iter().filter_map(Value::as_str).collect())
    .unwrap_or_default();
  if list.is_empty() {
    String::new()
  } else {
    format!("\nRemarques :\n- {}\n", list.join("\n- "))
  }
}

// -- outils ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Menus {
  api: Api,
  tool_router: ToolRouter<Self>,
}

#[tool_router]
impl Menus {
  fn new(api: Api) -> Self {
    Self {
      api,
      tool_router: Self::tool_router(),
    }
  }

  async fn summary(&self, week: &str) -> Result<String, String> {
    Ok(week_summary(&self.api.state(week).await?))
  }

  #[tool(
    description = "A week: household and evening settings, each day's brunch and dinner (recipe ids, durations, categories), leftovers, balance of the dinners, shopping-list progress",
    annotations(title = "Week", read_only_hint = true, open_world_hint = false)
  )]
  async fn get_week(&self, Parameters(p): Parameters<WeekParam>) -> CallToolResult {
    done(self.summary(p.week.as_deref().unwrap_or("")).await)
  }

  #[tool(
    description = "The family's recipes, one per line: id | name | minutes | category | starch | keeps | season | favourite | when last served",
    annotations(title = "Recipes", read_only_hint = true, open_world_hint = false)
  )]
  async fn list_recipes(&self, Parameters(p): Parameters<ListRecipesParams>) -> CallToolResult {
    let s = match self.api.state(p.week.as_deref().unwrap_or("")).await {
      Ok(s) => s,
      Err(e) => return refusal(e),
    };
    let fold = crate::slug::fold;
    let query = p.query.as_deref().map(fold).unwrap_or_default();
    let meal = p.meal.as_deref().map(fold);
    let mut out = String::new();
    let mut shown = 0;
    for r in &s.recipes {
      if r.excluded && !p.include_excluded.unwrap_or(false) {
        continue;
      }
      if p.favorites_only.unwrap_or(false) && !r.favorite {
        continue;
      }
      match (meal.as_deref(), r.meal) {
        (Some("dinner" | "souper"), Meal::Brunch) | (Some("brunch"), Meal::Dinner) => continue,
        _ => {}
      }
      if !query.is_empty() && !fold(&r.name).contains(&query) && !r.ingredients.iter().any(|l| fold(&l.name).contains(&query)) {
        continue;
      }
      let months = if r.months.is_empty() {
        "toute l'année".to_string()
      } else {
        format!("mois {}", r.months.iter().map(u32::to_string).collect::<Vec<_>>().join(","))
      };
      let _ = writeln!(
        out,
        "{} | {} | {} min{} | {} | {} | {} | {} | {}{}{} | {}",
        r.id,
        r.name,
        r.minutes,
        if r.active_minutes + 5 < r.minutes {
          format!(" ({} actives)", r.active_minutes)
        } else {
          String::new()
        },
        category(r),
        r.starch.as_deref().unwrap_or("-"),
        if r.keeps { "se garde" } else { "-" },
        months,
        if r.favorite { "♥" } else { "" },
        if r.excluded { " exclue" } else { "" },
        if r.favorite || r.excluded { "" } else { "-" },
        last_served(&s.history, &r.id)
      );
      shown += 1;
    }
    ok(format!("{shown} recette(s)\n{out}"))
  }

  #[tool(
    description = "One recipe: ingredients per adult and for the household, steps, notes, favourite and exclusion",
    annotations(title = "Recipe", read_only_hint = true, open_world_hint = false)
  )]
  async fn get_recipe(&self, Parameters(p): Parameters<IdParam>) -> CallToolResult {
    let value: Value = match self.api.json(reqwest::Method::GET, &format!("/api/recipes/{}", enc(&p.id)), None).await {
      Ok(v) => v,
      Err(e) => return refusal(e),
    };
    let Ok(r) = serde_json::from_value::<Recipe>(value["recipe"].clone()) else {
      return refusal("réponse inattendue de menus");
    };
    let mut out = format!("{} [{}]\n{} min ({} actives) · {}", r.name, r.id, r.minutes, r.active_minutes, category(&r));
    if let Some(starch) = &r.starch {
      let _ = write!(out, " · {starch}");
    }
    if r.keeps {
      out.push_str(" · se garde");
    }
    if !r.months.is_empty() {
      let _ = write!(out, " · mois {:?}", r.months);
    }
    if r.favorite {
      out.push_str(" · favori");
    }
    if r.excluded {
      out.push_str(" · exclue des propositions");
    }
    let _ = writeln!(
      out,
      "\n\nIngrédients (par adulte → pour le foyer, {} portions) :",
      number(value["portions"].as_f64().unwrap_or(1.0))
    );
    let lines = value["lines"].as_array().cloned().unwrap_or_default();
    for (line, scaled) in r.ingredients.iter().zip(lines.iter()) {
      let _ = writeln!(
        out,
        "- {} : {} {} → {}",
        line.name,
        number(line.qty),
        line.unit,
        scaled["amount"].as_str().unwrap_or("")
      );
    }
    out.push_str("\nPréparation :\n");
    for (i, step) in r.steps.iter().enumerate() {
      let _ = writeln!(out, "{}. {step}", i + 1);
    }
    if let Some(notes) = &r.notes {
      let _ = writeln!(out, "\nNote : {notes}");
    }
    ok(out)
  }

  #[tool(
    description = "The ingredient catalogue: name | unit | aisle | pantry. Recipes must use these names and units so the shopping list adds them up",
    annotations(title = "Ingredients", read_only_hint = true, open_world_hint = false)
  )]
  async fn list_ingredients(&self) -> CallToolResult {
    let value: Value = match self.api.json(reqwest::Method::GET, "/api/ingredients", None).await {
      Ok(v) => v,
      Err(e) => return refusal(e),
    };
    let settings: Settings = match self.api.json(reqwest::Method::GET, "/api/settings", None).await {
      Ok(s) => s,
      Err(e) => return refusal(e),
    };
    let mut out = String::from("Rayons : ");
    out.push_str(&settings.aisles.iter().map(|a| format!("{} = {}", a.id, a.name)).collect::<Vec<_>>().join(" ; "));
    out.push_str("\n\n");
    for item in value["items"].as_array().cloned().unwrap_or_default() {
      let _ = writeln!(
        out,
        "{} | {} | {}{}",
        item["name"].as_str().unwrap_or(""),
        item["unit"].as_str().unwrap_or(""),
        item["aisle"].as_str().unwrap_or(""),
        if item["pantry"].as_bool().unwrap_or(false) { " | placard" } else { "" }
      );
    }
    ok(out)
  }

  #[tool(
    description = "Write a whole week at once: every dinner and brunch, creating new recipes first if needed. Replaces the week's current dinners and brunches (the shopping list's ticks and added items are kept). Returns the week as saved",
    annotations(
      title = "Plan the week",
      read_only_hint = false,
      destructive_hint = true,
      idempotent_hint = true,
      open_world_hint = false
    )
  )]
  async fn plan_week(&self, Parameters(p): Parameters<PlanWeekParams>) -> CallToolResult {
    let d = p.dinners;
    let mut dinners = serde_json::Map::new();
    for (day, value) in [
      ("mon", d.mon),
      ("tue", d.tue),
      ("wed", d.wed),
      ("thu", d.thu),
      ("fri", d.fri),
      ("sat", d.sat),
      ("sun", d.sun),
    ] {
      if let Some(slot) = value.as_deref().and_then(dinner_slot) {
        dinners.insert(day.into(), slot);
      }
    }
    let b = p.brunch.unwrap_or_default();
    let mut brunch = serde_json::Map::new();
    for (day, value) in [
      ("mon", b.mon),
      ("tue", b.tue),
      ("wed", b.wed),
      ("thu", b.thu),
      ("fri", b.fri),
      ("sat", b.sat),
      ("sun", b.sun),
    ] {
      if let Some(ids) = value.filter(|ids| !ids.is_empty()) {
        brunch.insert(day.into(), json!(ids));
      }
    }
    let body = json!({ "dinners": dinners, "brunch": brunch, "new_recipes": p.new_recipes.unwrap_or_default(), "note": p.note });
    let result: Value = match self
      .api
      .json(reqwest::Method::POST, &format!("/api/weeks/{}/apply", enc(&p.week)), Some(body))
      .await
    {
      Ok(v) => v,
      Err(e) => return refusal(e),
    };
    let created = result["created"]
      .as_object()
      .map(|m| m.iter().map(|(k, v)| format!("{k} → {}", v.as_str().unwrap_or(""))).collect::<Vec<_>>())
      .unwrap_or_default();
    let week = result["week"]["week"].as_str().unwrap_or(&p.week).to_string();
    let mut out = match self.summary(&week).await {
      Ok(s) => s,
      Err(e) => return refusal(e),
    };
    if !created.is_empty() {
      let _ = write!(out, "\nRecettes créées : {}\n", created.join(", "));
    }
    out.push_str(&warnings_text(&result));
    ok(out)
  }

  #[tool(
    description = "Set one evening's dinner: a recipe id, \"leftovers\" (the last dish cooked before, made twice as much), \"free\" or \"empty\"",
    annotations(
      title = "Set a dinner",
      read_only_hint = false,
      destructive_hint = false,
      idempotent_hint = true,
      open_world_hint = false
    )
  )]
  async fn set_dinner(&self, Parameters(p): Parameters<DinnerParams>) -> CallToolResult {
    let path = format!("/api/weeks/{}/dinners/{}", enc(&p.week), enc(&p.day));
    if let Err(e) = self.api.send(reqwest::Method::PUT, &path, Some(json!(p.value.trim()))).await {
      return refusal(e);
    }
    done(self.summary(&p.week).await)
  }

  #[tool(
    description = "Set one day's brunch: the list of recipe ids, in order (an empty list removes it)",
    annotations(
      title = "Set a brunch",
      read_only_hint = false,
      destructive_hint = false,
      idempotent_hint = true,
      open_world_hint = false
    )
  )]
  async fn set_brunch(&self, Parameters(p): Parameters<BrunchParams>) -> CallToolResult {
    let path = format!("/api/weeks/{}/brunch/{}", enc(&p.week), enc(&p.day));
    if let Err(e) = self.api.send(reqwest::Method::PUT, &path, Some(json!({ "recipes": p.recipes }))).await {
      return refusal(e);
    }
    done(self.summary(&p.week).await)
  }

  #[tool(
    description = "Let the app draw a plan by itself (weighted random choice following the balance rules, season, leftovers and history). Fast; a starting point to adjust",
    annotations(
      title = "Quick proposal",
      read_only_hint = false,
      destructive_hint = false,
      idempotent_hint = false,
      open_world_hint = false
    )
  )]
  async fn propose_week(&self, Parameters(p): Parameters<ProposeParams>) -> CallToolResult {
    let path = format!("/api/weeks/{}/propose", enc(&p.week));
    if let Err(e) = self
      .api
      .send(reqwest::Method::POST, &path, Some(json!({ "keep": p.keep.unwrap_or(true) })))
      .await
    {
      return refusal(e);
    }
    done(self.summary(&p.week).await)
  }

  #[tool(
    description = "Replace one evening's dinner with another recipe the app picks, the rest of the week unchanged",
    annotations(
      title = "Another dinner",
      read_only_hint = false,
      destructive_hint = false,
      idempotent_hint = false,
      open_world_hint = false
    )
  )]
  async fn reroll_dinner(&self, Parameters(p): Parameters<DayParams>) -> CallToolResult {
    let path = format!("/api/weeks/{}/reroll/{}", enc(&p.week), enc(&p.day));
    if let Err(e) = self.api.send(reqwest::Method::POST, &path, None).await {
      return refusal(e);
    }
    done(self.summary(&p.week).await)
  }

  #[tool(
    description = "Add recipes to the family's collection. Returns their ids and the remarks to read (new ingredients, units that differ from the catalogue)",
    annotations(
      title = "Add recipes",
      read_only_hint = false,
      destructive_hint = false,
      idempotent_hint = false,
      open_world_hint = false
    )
  )]
  async fn add_recipes(&self, Parameters(p): Parameters<AddRecipesParams>) -> CallToolResult {
    let result: Value = match self
      .api
      .json(reqwest::Method::POST, "/api/recipes", Some(json!({ "recipes": p.recipes, "source": "claude" })))
      .await
    {
      Ok(v) => v,
      Err(e) => return refusal(e),
    };
    let created: Vec<String> = result["created"]
      .as_array()
      .map(|list| {
        list
          .iter()
          .map(|r| format!("{} [{}]", r["name"].as_str().unwrap_or(""), r["id"].as_str().unwrap_or("")))
          .collect()
      })
      .unwrap_or_default();
    ok(format!("Ajoutée(s) : {}{}", created.join(", "), warnings_text(&result)))
  }

  #[tool(
    description = "Replace a recipe entirely (name, ingredients, steps…). Favourite and exclusion are kept",
    annotations(
      title = "Update a recipe",
      read_only_hint = false,
      destructive_hint = true,
      idempotent_hint = true,
      open_world_hint = false
    )
  )]
  async fn update_recipe(&self, Parameters(p): Parameters<UpdateRecipeParams>) -> CallToolResult {
    let mut body = serde_json::to_value(&p.recipe).unwrap_or(Value::Null);
    if let Some(map) = body.as_object_mut() {
      map.remove("id");
    }
    match self
      .api
      .json::<Value>(reqwest::Method::PUT, &format!("/api/recipes/{}", enc(&p.id)), Some(body))
      .await
    {
      Ok(result) => ok(format!("Recette {} remplacée.{}", p.id, warnings_text(&result))),
      Err(e) => refusal(e),
    }
  }

  #[tool(
    description = "Mark a recipe as favourite (proposed more often) or excluded (never proposed), or set its note",
    annotations(
      title = "Favourite / exclude",
      read_only_hint = false,
      destructive_hint = false,
      idempotent_hint = true,
      open_world_hint = false
    )
  )]
  async fn set_recipe_flags(&self, Parameters(p): Parameters<FlagsParams>) -> CallToolResult {
    let body = json!({ "favorite": p.favorite, "excluded": p.excluded, "notes": p.notes });
    match self
      .api
      .json::<Recipe>(reqwest::Method::PATCH, &format!("/api/recipes/{}", enc(&p.id)), Some(body))
      .await
    {
      Ok(r) => ok(format!(
        "{} : {}, {}.",
        r.name,
        if r.favorite { "favori" } else { "pas favori" },
        if r.excluded { "exclue des propositions" } else { "proposée" }
      )),
      Err(e) => refusal(e),
    }
  }

  #[tool(
    description = "Delete a recipe; it is also removed from the coming weeks. Only when the user asks",
    annotations(
      title = "Delete a recipe",
      read_only_hint = false,
      destructive_hint = true,
      idempotent_hint = true,
      open_world_hint = false
    )
  )]
  async fn delete_recipe(&self, Parameters(p): Parameters<IdParam>) -> CallToolResult {
    match self
      .api
      .json::<Value>(reqwest::Method::DELETE, &format!("/api/recipes/{}", enc(&p.id)), None)
      .await
    {
      Ok(v) => ok(format!(
        "Recette {} supprimée. Semaines mises à jour : {}.",
        p.id,
        v["weeks_updated"]
          .as_array()
          .map(|w| w.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
          .filter(|s| !s.is_empty())
          .unwrap_or("aucune".into())
      )),
      Err(e) => refusal(e),
    }
  }

  #[tool(
    description = "The shopping list as text, aisle by aisle in store order, with the items still to buy (ticked ones left out); pantry items to check at the end",
    annotations(title = "Shopping list", read_only_hint = true, open_world_hint = false)
  )]
  async fn get_shopping_list(&self, Parameters(p): Parameters<ListParams>) -> CallToolResult {
    let week = p.week.unwrap_or_default();
    let week = if week.is_empty() { "courante".to_string() } else { week };
    done(
      self
        .api
        .get(&format!(
          "/api/weeks/{}/list?format=text&days={}",
          enc(&week),
          enc(p.days.as_deref().unwrap_or(""))
        ))
        .await,
    )
  }

  #[tool(
    description = "Add items by hand to a week's shopping list (milk, toilet paper…)",
    annotations(
      title = "Add to the list",
      read_only_hint = false,
      destructive_hint = false,
      idempotent_hint = false,
      open_world_hint = false
    )
  )]
  async fn add_to_shopping_list(&self, Parameters(p): Parameters<ExtrasParams>) -> CallToolResult {
    match self
      .api
      .json::<Value>(
        reqwest::Method::POST,
        &format!("/api/weeks/{}/extras", enc(&p.week)),
        Some(json!({ "text": p.items })),
      )
      .await
    {
      Ok(v) => ok(format!(
        "Ajouté : {}.",
        v.as_array()
          .map(|a| a.iter().filter_map(|e| e["text"].as_str()).collect::<Vec<_>>().join(", "))
          .unwrap_or_default()
      )),
      Err(e) => refusal(e),
    }
  }

  #[tool(
    description = "Change household or evening settings: adults, children, child_factor, long_days, leftover_days, free_days, brunch_days, brunch_base, quick_minutes, avoid_weeks, store_name. Only the fields given change",
    annotations(
      title = "Settings",
      read_only_hint = false,
      destructive_hint = false,
      idempotent_hint = true,
      open_world_hint = false
    )
  )]
  async fn update_settings(&self, Parameters(p): Parameters<SettingsIn>) -> CallToolResult {
    let body = serde_json::to_value(&p).unwrap_or(Value::Null);
    if body.as_object().is_none_or(|m| m.is_empty()) {
      return refusal("aucun réglage fourni");
    }
    match self.api.json::<Settings>(reqwest::Method::PATCH, "/api/settings", Some(body)).await {
      Ok(s) => ok(format!(
        "Réglages enregistrés : {} adulte(s), {} enfant(s) à {} ; soirs longs {}, restes {}, libres {} ; brunchs {} ; plat rapide {} min ; pas de répétition avant {} semaines.",
        s.adults,
        s.children,
        number(s.child_factor),
        days(&s.long_days),
        days(&s.leftover_days),
        days(&s.free_days),
        days(&s.brunch_days),
        s.quick_minutes,
        s.avoid_weeks
      )),
      Err(e) => refusal(e),
    }
  }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Menus {
  fn get_info(&self) -> ServerConfig {
    ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
      .with_server_info(Implementation::new("menus", env!("CARGO_PKG_VERSION")))
      .with_instructions(INSTRUCTIONS)
  }
}

/// Les outils, branchés sur une instance : pour le chat de l'app, qui les sert en interne.
pub(crate) fn tools_for(base: &str, token: Option<String>) -> anyhow::Result<Menus> {
  let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).no_proxy().build()?;
  Ok(Menus::new(Api {
    http,
    base: base.trim_end_matches('/').to_string(),
    token,
  }))
}

/// Sert le protocole MCP sur stdin/stdout jusqu'à ce que l'assistant ferme la connexion.
pub async fn run(url: String, token: Option<String>) -> anyhow::Result<()> {
  // Les assistants lancent parfois leurs serveurs MCP derrière un proxy (HTTP_PROXY) :
  // l'instance est sur le réseau local, on y va toujours en direct.
  let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).no_proxy().build()?;
  let api = Api {
    http,
    base: url.trim_end_matches('/').to_string(),
    token,
  };
  tracing::info!("menus mcp : relais vers {}", api.base);
  let service = Menus::new(api).serve(rmcp::transport::stdio()).await?;
  service.waiting().await?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn tools_are_listed() {
    let api = Api {
      http: reqwest::Client::new(),
      base: "http://localhost:1".into(),
      token: None,
    };
    let menus = Menus::new(api);
    let names: Vec<String> = menus.tool_router.list_all().into_iter().map(|t| t.name.to_string()).collect();
    for expected in ["get_week", "list_recipes", "plan_week", "add_recipes", "get_shopping_list", "update_settings"] {
      assert!(names.iter().any(|n| n == expected), "{expected} manque dans {names:?}");
    }
  }

  #[test]
  fn dinner_values_become_slots() {
    assert_eq!(dinner_slot("restes"), Some(json!({ "kind": "leftovers" })));
    assert_eq!(dinner_slot("free"), Some(json!({ "kind": "free" })));
    assert_eq!(dinner_slot("empty"), None);
    assert_eq!(dinner_slot(" carbonara "), Some(json!({ "kind": "recipe", "recipe": "carbonara" })));
  }
}
