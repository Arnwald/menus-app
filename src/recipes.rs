//! Brouillons de recettes (saisie manuelle, Claude, fichiers de départ) et
//! leur validation contre le catalogue d'ingrédients.

use crate::model::{BrunchKind, Catalog, Ingredient, Line, Meal, Protein, Recipe, Settings, Unit};
use crate::slug::{fold, slugify, unique};
use serde::{Deserialize, Serialize};

/// Ligne d'ingrédient telle qu'on l'écrit : texte libre (`"oignon 0.5 pce"`,
/// `"200 g de farine"`, `"2 oignons"`) ou objet.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DraftLine {
  Text(String),
  Object {
    name: String,
    qty: f64,
    unit: String,
    /// Rayon, pour un ingrédient absent du catalogue.
    #[serde(default)]
    aisle: Option<String>,
    #[serde(default)]
    pantry: Option<bool>,
  },
}

/// Recette à créer ou à remplacer. Tout est facultatif sauf le nom et les
/// ingrédients ; les valeurs manquantes prennent un défaut raisonnable.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecipeDraft {
  #[serde(default)]
  pub id: Option<String>,
  pub name: String,
  #[serde(default)]
  pub meal: Option<Meal>,
  #[serde(default)]
  pub brunch_kind: Option<BrunchKind>,
  #[serde(default)]
  pub minutes: Option<u32>,
  #[serde(default)]
  pub active_minutes: Option<u32>,
  #[serde(default)]
  pub protein: Option<Protein>,
  #[serde(default)]
  pub starch: Option<String>,
  #[serde(default)]
  pub keeps: Option<bool>,
  #[serde(default)]
  pub months: Option<Vec<u32>>,
  #[serde(default)]
  pub ingredients: Vec<DraftLine>,
  #[serde(default)]
  pub steps: Vec<String>,
  #[serde(default)]
  pub notes: Option<String>,
  #[serde(default)]
  pub favorite: Option<bool>,
  #[serde(default)]
  pub excluded: Option<bool>,
}

impl From<&Recipe> for RecipeDraft {
  fn from(r: &Recipe) -> Self {
    RecipeDraft {
      id: Some(r.id.clone()),
      name: r.name.clone(),
      meal: Some(r.meal),
      brunch_kind: r.brunch_kind,
      minutes: Some(r.minutes),
      active_minutes: Some(r.active_minutes),
      protein: r.protein,
      starch: r.starch.clone(),
      keeps: Some(r.keeps),
      months: Some(r.months.clone()),
      ingredients: r
        .ingredients
        .iter()
        .map(|l| DraftLine::Object {
          name: l.name.clone(),
          qty: l.qty,
          unit: l.unit.code().into(),
          aisle: None,
          pantry: None,
        })
        .collect(),
      steps: r.steps.clone(),
      notes: r.notes.clone(),
      favorite: Some(r.favorite),
      excluded: Some(r.excluded),
    }
  }
}

/// Ingrédient inconnu du catalogue, ajouté à l'enregistrement de la recette.
#[derive(Debug, Clone, Serialize)]
pub struct NewIngredient {
  pub name: String,
  #[serde(flatten)]
  pub ingredient: Ingredient,
}

/// Résultat d'une validation : la recette, les ingrédients à ajouter au
/// catalogue, et les remarques à montrer.
#[derive(Debug, Clone, Serialize)]
pub struct Checked {
  pub recipe: Recipe,
  pub new_ingredients: Vec<NewIngredient>,
  pub warnings: Vec<String>,
}

/// Lit une quantité : `0.5`, `0,5`, `1/2`, `1½`.
pub fn parse_qty(s: &str) -> Option<f64> {
  let s = s.trim().replace(',', ".").replace('½', ".5").replace('¼', ".25").replace('¾', ".75");
  let s = if s.starts_with('.') { format!("0{s}") } else { s };
  if let Some((a, b)) = s.split_once('/') {
    let (a, b): (f64, f64) = (a.parse().ok()?, b.parse().ok()?);
    return (b != 0.0).then_some(a / b);
  }
  s.parse::<f64>().ok().filter(|q| q.is_finite())
}

/// Découpe une ligne écrite librement en (nom, quantité, unité). Formes
/// acceptées : `nom quantité unité`, `nom quantité`, `quantité unité nom`,
/// `quantité nom`, avec `de`/`d'` facultatif après l'unité.
pub fn parse_line(text: &str) -> Result<(String, f64, Unit), String> {
  let tokens: Vec<&str> = text.split_whitespace().collect();
  let bad = || format!("ligne illisible : « {text} » (attendu « nom quantité unité »)");
  let clean_name = |parts: &[&str]| -> String {
    let name = parts.join(" ");
    let name = name
      .strip_prefix("de ")
      .or_else(|| name.strip_prefix("d'"))
      .or_else(|| name.strip_prefix("d’"))
      .unwrap_or(&name);
    name.trim().to_string()
  };

  if tokens.len() < 2 {
    return Err(bad());
  }

  // Quantité en tête : « 200 g de farine », « 2 oignons ».
  if let Some(qty) = parse_qty(tokens[0]) {
    // Unité collée (« 200g ») ou séparée.
    if let Some((unit, factor)) = Unit::parse(tokens[1]) {
      let name = clean_name(&tokens[2..]);
      return if name.is_empty() { Err(bad()) } else { Ok((name, qty * factor, unit)) };
    }
    return Ok((clean_name(&tokens[1..]), qty, Unit::Pce));
  }
  if let Some((number, rest)) = split_glued(tokens[0]) {
    if let Some((unit, factor)) = Unit::parse(rest) {
      let name = clean_name(&tokens[1..]);
      return if name.is_empty() { Err(bad()) } else { Ok((name, number * factor, unit)) };
    }
  }

  // Quantité en fin : « oignon 0.5 pce », « oignon 2 », « farine 200g ».
  let last = tokens[tokens.len() - 1];
  if let Some((unit, factor)) = Unit::parse(last) {
    if tokens.len() >= 3 {
      if let Some(qty) = parse_qty(tokens[tokens.len() - 2]) {
        return Ok((tokens[..tokens.len() - 2].join(" "), qty * factor, unit));
      }
    }
  }
  if let Some(qty) = parse_qty(last) {
    return Ok((tokens[..tokens.len() - 1].join(" "), qty, Unit::Pce));
  }
  if let Some((number, rest)) = split_glued(last) {
    if let Some((unit, factor)) = Unit::parse(rest) {
      return Ok((tokens[..tokens.len() - 1].join(" "), number * factor, unit));
    }
  }
  Err(bad())
}

/// « 200g » → (200, "g").
fn split_glued(token: &str) -> Option<(f64, &str)> {
  let pos = token.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))?;
  if pos == 0 {
    return None;
  }
  Some((parse_qty(&token[..pos])?, &token[pos..]))
}

/// Valide un brouillon. `taken` dit si un identifiant est déjà pris ; `keep_id`
/// impose l'identifiant (remplacement d'une recette existante).
pub fn check(
  draft: &RecipeDraft,
  catalog: &Catalog,
  settings: &Settings,
  taken: impl Fn(&str) -> bool,
  keep_id: Option<&str>,
  source: &str,
) -> Result<Checked, String> {
  let name = draft.name.trim();
  if name.is_empty() {
    return Err("une recette doit avoir un nom".into());
  }
  if draft.ingredients.is_empty() {
    return Err(format!("« {name} » n'a aucun ingrédient"));
  }

  let mut warnings = vec![];
  let mut new_ingredients: Vec<NewIngredient> = vec![];
  let mut lines: Vec<Line> = vec![];

  for raw in &draft.ingredients {
    let (raw_name, qty, unit, aisle, pantry) = match raw {
      DraftLine::Text(text) => {
        let (n, q, u) = parse_line(text)?;
        (n, q, u, None, None)
      }
      DraftLine::Object {
        name,
        qty,
        unit,
        aisle,
        pantry,
      } => {
        let (unit, factor) = Unit::parse(unit).ok_or_else(|| format!("unité inconnue « {unit} » pour {name}"))?;
        (name.trim().to_string(), qty * factor, unit, aisle.clone(), *pantry)
      }
    };
    if raw_name.is_empty() {
      return Err("un ingrédient n'a pas de nom".into());
    }
    if qty.is_nan() || qty <= 0.0 {
      return Err(format!("quantité nulle ou négative pour {raw_name}"));
    }

    let canonical = match catalog.resolve(&raw_name) {
      Some((known, ingredient)) => {
        if ingredient.unit != unit {
          warnings.push(format!(
            "{known} : le catalogue compte en « {} », la recette en « {} » ; la liste de courses les affichera séparément",
            ingredient.unit, unit
          ));
        }
        known.to_string()
      }
      None => {
        let already = new_ingredients.iter().any(|n| fold(&n.name) == fold(&raw_name));
        if !already {
          let aisle = aisle.filter(|a| settings.has_aisle(a)).unwrap_or_else(|| "di".into());
          warnings.push(format!("nouvel ingrédient « {raw_name} », rangé dans « {} »", settings.aisle_name(&aisle)));
          new_ingredients.push(NewIngredient {
            name: raw_name.clone(),
            ingredient: Ingredient {
              aisle,
              unit,
              pantry: pantry.unwrap_or(false),
            },
          });
        }
        raw_name
      }
    };

    // Une même ligne écrite deux fois s'additionne.
    match lines.iter_mut().find(|l| l.name == canonical && l.unit == unit) {
      Some(line) => line.qty += qty,
      None => lines.push(Line { name: canonical, qty, unit }),
    }
  }

  let meal = draft.meal.unwrap_or(if draft.brunch_kind.is_some() { Meal::Brunch } else { Meal::Dinner });
  let minutes = draft.minutes.filter(|m| *m > 0).unwrap_or(30);
  let active_minutes = draft.active_minutes.filter(|m| *m > 0).unwrap_or(minutes).min(minutes);
  let mut months = draft.months.clone().unwrap_or_default();
  months.retain(|m| (1..=12).contains(m));
  months.sort();
  months.dedup();
  if months.len() == 12 {
    months.clear();
  }
  if meal == Meal::Dinner && draft.protein.is_none() {
    warnings.push(format!(
      "« {name} » : catégorie (poisson, viande…) non précisée, le plat compte comme végétarien pour l'équilibre"
    ));
  }

  let id = match keep_id {
    Some(id) => id.to_string(),
    None => unique(&slugify(draft.id.as_deref().filter(|i| !i.starts_with("new")).unwrap_or(name)), &taken),
  };
  let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();

  let recipe = Recipe {
    id,
    name: name.to_string(),
    meal,
    brunch_kind: if meal == Meal::Brunch {
      Some(draft.brunch_kind.unwrap_or(BrunchKind::Savory))
    } else {
      None
    },
    minutes,
    active_minutes,
    protein: if meal == Meal::Dinner {
      Some(draft.protein.unwrap_or(Protein::Veggie))
    } else {
      draft.protein
    },
    starch: draft.starch.as_ref().map(|s| fold(s)).filter(|s| !s.is_empty()),
    keeps: draft.keeps.unwrap_or(false),
    months,
    ingredients: lines,
    steps: draft.steps.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
    favorite: draft.favorite.unwrap_or(false),
    excluded: draft.excluded.unwrap_or(false),
    notes: draft.notes.clone().filter(|n| !n.trim().is_empty()),
    source: source.to_string(),
    created_at: now.clone(),
    updated_at: now,
  };
  Ok(Checked {
    recipe,
    new_ingredients,
    warnings,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_free_lines() {
    assert_eq!(parse_line("oignon 0.5 pce").unwrap(), ("oignon".into(), 0.5, Unit::Pce));
    assert_eq!(parse_line("tresse (Zopf) 80 g").unwrap(), ("tresse (Zopf)".into(), 80.0, Unit::G));
    assert_eq!(parse_line("200 g de farine").unwrap(), ("farine".into(), 200.0, Unit::G));
    assert_eq!(parse_line("2 oignons").unwrap(), ("oignons".into(), 2.0, Unit::Pce));
    assert_eq!(parse_line("1,5 dl lait").unwrap(), ("lait".into(), 150.0, Unit::Ml));
    assert_eq!(parse_line("beurre 1/2 cs").unwrap(), ("beurre".into(), 0.5, Unit::Cs));
    assert_eq!(parse_line("0.5 kg pommes de terre").unwrap(), ("pommes de terre".into(), 500.0, Unit::G));
    assert_eq!(parse_line("farine 200g").unwrap(), ("farine".into(), 200.0, Unit::G));
    assert_eq!(parse_line("200g farine").unwrap(), ("farine".into(), 200.0, Unit::G));
    assert!(parse_line("sel").is_err());
  }
}
