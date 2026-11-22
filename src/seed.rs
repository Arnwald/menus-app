//! Base de départ embarquée dans le binaire (`seed/*.toml`).

use crate::model::{Catalog, Ingredient, Recipe, Settings};
use crate::recipes::{check, DraftLine, RecipeDraft};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;

const CATALOG: &str = include_str!("../seed/catalogue.toml");
const RECIPES: &str = include_str!("../seed/recettes.toml");

#[derive(Deserialize)]
struct CatalogFile {
  ingredients: BTreeMap<String, SeedIngredient>,
}

#[derive(Deserialize)]
struct SeedIngredient {
  r: String,
  u: String,
  #[serde(default)]
  p: bool,
}

#[derive(Deserialize)]
struct RecipesFile {
  recipe: Vec<SeedRecipe>,
}

#[derive(Deserialize)]
struct SeedRecipe {
  id: String,
  name: String,
  meal: crate::model::Meal,
  #[serde(default)]
  brunch_kind: Option<crate::model::BrunchKind>,
  minutes: u32,
  #[serde(default)]
  active: Option<u32>,
  #[serde(default)]
  protein: Option<crate::model::Protein>,
  #[serde(default)]
  starch: Option<String>,
  #[serde(default)]
  keeps: bool,
  #[serde(default)]
  months: Vec<u32>,
  ing: Vec<String>,
  #[serde(default)]
  steps: Vec<String>,
}

pub fn catalog() -> Result<Catalog> {
  let file: CatalogFile = toml::from_str(CATALOG).context("seed/catalogue.toml")?;
  let mut items = BTreeMap::new();
  for (name, raw) in file.ingredients {
    let (unit, factor) = crate::model::Unit::parse(&raw.u).with_context(|| format!("unité inconnue pour {name}"))?;
    if factor != 1.0 {
      bail!("{name} : l'unité de référence doit être g, ml, pce… (pas {})", raw.u);
    }
    items.insert(
      name,
      Ingredient {
        aisle: raw.r,
        unit,
        pantry: raw.p,
      },
    );
  }
  Ok(Catalog { items })
}

/// Recettes de départ, validées strictement : tout ingrédient doit exister dans
/// le catalogue, avec la même unité.
pub fn recipes(catalog: &Catalog, settings: &Settings) -> Result<Vec<Recipe>> {
  let file: RecipesFile = toml::from_str(RECIPES).context("seed/recettes.toml")?;
  let mut out: Vec<Recipe> = vec![];
  for seed in file.recipe {
    let draft = RecipeDraft {
      id: Some(seed.id.clone()),
      name: seed.name,
      meal: Some(seed.meal),
      brunch_kind: seed.brunch_kind,
      minutes: Some(seed.minutes),
      active_minutes: seed.active,
      protein: seed.protein,
      starch: seed.starch,
      keeps: Some(seed.keeps),
      months: Some(seed.months),
      ingredients: seed.ing.into_iter().map(DraftLine::Text).collect(),
      steps: seed.steps,
      ..Default::default()
    };
    if out.iter().any(|r| r.id == seed.id) {
      bail!("identifiant en double dans seed/recettes.toml : {}", seed.id);
    }
    let checked = check(&draft, catalog, settings, |_| false, Some(&seed.id), "base").map_err(anyhow::Error::msg)?;
    if !checked.new_ingredients.is_empty() || !checked.warnings.is_empty() {
      bail!("{} : {}", seed.id, checked.warnings.join(" ; "));
    }
    out.push(checked.recipe);
  }
  Ok(out)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::model::{Meal, Protein};

  #[test]
  fn seed_is_consistent() {
    let settings = Settings::default();
    let catalog = catalog().unwrap();
    for (name, ing) in &catalog.items {
      assert!(settings.has_aisle(&ing.aisle), "{name} : rayon inconnu {}", ing.aisle);
    }
    let recipes = recipes(&catalog, &settings).unwrap();
    let dinners: Vec<_> = recipes.iter().filter(|r| r.meal == Meal::Dinner).collect();
    assert!(dinners.len() >= 40, "{} soupers", dinners.len());
    assert!(recipes.iter().any(|r| r.id == "base-brunch"));
    for protein in [Protein::Fish, Protein::Poultry, Protein::Meat, Protein::Veggie, Protein::Legumes] {
      let n = dinners
        .iter()
        .filter(|r| r.protein == Some(protein) && r.in_season(11) && r.is_quick(35))
        .count();
      assert!(n >= 3, "seulement {n} soupers rapides de novembre en {protein:?}");
    }
  }
}
