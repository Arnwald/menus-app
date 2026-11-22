//! Accès à la base « en clair ».
//!
//! Les fichiers JSON sont la source de vérité : chaque lecture relit le disque,
//! si bien qu'une modification faite à la main (ou par Claude) est prise en
//! compte immédiatement. Les écritures sont atomiques (fichier temporaire puis
//! `rename`).
//!
//! ```text
//! data/
//!   settings.json        foyer, soirs particuliers, rayons
//!   ingredients.json     catalogue : rayon, unité, placard
//!   recipes.json         les recettes
//!   weeks/2026-W47.json  soupers, brunchs, cases cochées, ajouts
//! ```

use crate::model::{Catalog, Recipe, Settings, Week};
use crate::weekid::WeekId;
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::broadcast;

/// Évènement diffusé aux navigateurs connectés (SSE).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Change {
  /// `week`, `recipes`, `ingredients`, `settings` ou `all`.
  pub scope: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub week: Option<String>,
}

impl Change {
  pub fn scope(scope: &str) -> Self {
    Change {
      scope: scope.to_string(),
      week: None,
    }
  }
  pub fn week(week: WeekId) -> Self {
    Change {
      scope: "week".to_string(),
      week: Some(week.to_string()),
    }
  }
}

#[derive(Clone)]
pub struct Store {
  root: PathBuf,
  tx: broadcast::Sender<Change>,
}

impl Store {
  /// Ouvre la base dans `root` ; au premier lancement, y dépose les réglages
  /// par défaut, le catalogue et les recettes de départ.
  pub fn open(root: impl AsRef<Path>) -> Result<Self> {
    let root = root.as_ref().to_path_buf();
    std::fs::create_dir_all(root.join("weeks")).with_context(|| format!("création de {}", root.display()))?;
    let (tx, _) = broadcast::channel(64);
    let store = Store { root, tx };

    if !store.settings_path().exists() {
      store.save_settings(&Settings::default())?;
    }
    if !store.ingredients_path().exists() {
      store.save_catalog(&crate::seed::catalog()?)?;
    }
    if !store.recipes_path().exists() {
      let mut recipes = crate::seed::recipes(&store.catalog()?, &store.settings()?)?;
      store.save_recipes(&mut recipes)?;
    }
    Ok(store)
  }

  pub fn root(&self) -> &Path {
    &self.root
  }

  // -- diffusion ------------------------------------------------------------

  pub fn subscribe(&self) -> broadcast::Receiver<Change> {
    self.tx.subscribe()
  }

  pub fn notify(&self, change: Change) {
    let _ = self.tx.send(change);
  }

  // -- chemins --------------------------------------------------------------

  pub fn settings_path(&self) -> PathBuf {
    self.root.join("settings.json")
  }
  pub fn ingredients_path(&self) -> PathBuf {
    self.root.join("ingredients.json")
  }
  pub fn recipes_path(&self) -> PathBuf {
    self.root.join("recipes.json")
  }
  pub fn week_path(&self, week: WeekId) -> PathBuf {
    self.root.join("weeks").join(format!("{week}.json"))
  }

  // -- lecture / écriture ---------------------------------------------------

  pub fn settings(&self) -> Result<Settings> {
    read_json(&self.settings_path()).map(|s| s.unwrap_or_default())
  }

  pub fn save_settings(&self, settings: &Settings) -> Result<()> {
    write_json(&self.settings_path(), settings)
  }

  pub fn catalog(&self) -> Result<Catalog> {
    read_json(&self.ingredients_path()).map(|c| c.unwrap_or_default())
  }

  pub fn save_catalog(&self, catalog: &Catalog) -> Result<()> {
    write_json(&self.ingredients_path(), catalog)
  }

  pub fn recipes(&self) -> Result<Vec<Recipe>> {
    read_json(&self.recipes_path()).map(|r| r.unwrap_or_default())
  }

  /// Écrit les recettes triées (soupers puis brunchs, par nom) pour des diffs lisibles.
  pub fn save_recipes(&self, recipes: &mut Vec<Recipe>) -> Result<()> {
    recipes.sort_by_key(|r| (r.meal as u8, crate::slug::fold(&r.name)));
    write_json(&self.recipes_path(), recipes)
  }

  pub fn week(&self, week: WeekId) -> Result<Week> {
    Ok(read_json(&self.week_path(week))?.unwrap_or_else(|| Week::empty(week)))
  }

  /// Écrit une semaine ; une semaine redevenue vide est supprimée.
  pub fn save_week(&self, week: &mut Week) -> Result<()> {
    let path = self.week_path(week.week);
    if week.is_blank() {
      if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("suppression de {}", path.display()))?;
      }
      return Ok(());
    }
    week.updated_at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    write_json(&path, week)
  }

  /// Semaines présentes sur le disque, triées.
  pub fn week_ids(&self) -> Result<Vec<WeekId>> {
    let mut ids = vec![];
    for entry in std::fs::read_dir(self.root.join("weeks"))? {
      let name = entry?.file_name().to_string_lossy().to_string();
      if let Some(id) = name.strip_suffix(".json").and_then(|s| s.parse::<WeekId>().ok()) {
        ids.push(id);
      }
    }
    ids.sort();
    Ok(ids)
  }

  /// Pour chaque recette, l'écart (en semaines, valeur absolue) avec la
  /// semaine la plus proche où elle figure, `around` exclue.
  pub fn history(&self, around: WeekId) -> Result<HashMap<String, u32>> {
    let mut distance: HashMap<String, u32> = HashMap::new();
    for id in self.week_ids()? {
      if id == around {
        continue;
      }
      let gap = around.since(id).unsigned_abs() as u32;
      if gap > 12 {
        continue;
      }
      let week = self.week(id)?;
      for recipe in week.recipe_ids() {
        let entry = distance.entry(recipe.to_string()).or_insert(gap);
        *entry = (*entry).min(gap);
      }
    }
    Ok(distance)
  }
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
  match std::fs::read_to_string(path) {
    Ok(text) => serde_json::from_str(&text).map(Some).with_context(|| format!("lecture de {}", path.display())),
    Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
    Err(err) => Err(err).with_context(|| format!("lecture de {}", path.display())),
  }
}

/// Écriture atomique : fichier temporaire, puis `rename`.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
  let mut text = serde_json::to_string_pretty(value)?;
  text.push('\n');
  let tmp = path.with_extension("json.tmp");
  std::fs::write(&tmp, text).with_context(|| format!("écriture de {}", tmp.display()))?;
  std::fs::rename(&tmp, path).with_context(|| format!("remplacement de {}", path.display()))?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::model::{Day, Slot};

  #[test]
  fn opens_with_seed_and_round_trips_weeks() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    assert!(store.recipes().unwrap().len() > 50);
    assert!(store.catalog().unwrap().items.contains_key("oignon"));

    let id: WeekId = "2026-W47".parse().unwrap();
    let mut week = store.week(id).unwrap();
    week.set_dinner(Day::Mon, Slot::recipe("carbonara"));
    store.save_week(&mut week).unwrap();
    assert_eq!(store.week(id).unwrap().dinner(Day::Mon), &Slot::recipe("carbonara"));
    assert_eq!(store.week_ids().unwrap(), vec![id]);

    let history = store.history(id.add(2)).unwrap();
    assert_eq!(history.get("carbonara"), Some(&2));

    week.set_dinner(Day::Mon, Slot::Empty);
    store.save_week(&mut week).unwrap();
    assert!(store.week_ids().unwrap().is_empty());
  }
}
