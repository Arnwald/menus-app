//! Types de la base : recettes, catalogue d'ingrédients, réglages, semaines.

use crate::slug::fold;
use crate::weekid::WeekId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

// -- jours ------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Day {
  #[serde(alias = "lun", alias = "lundi")]
  Mon,
  #[serde(alias = "mar", alias = "mardi")]
  Tue,
  #[serde(alias = "mer", alias = "mercredi")]
  Wed,
  #[serde(alias = "jeu", alias = "jeudi")]
  Thu,
  #[serde(alias = "ven", alias = "vendredi")]
  Fri,
  #[serde(alias = "sam", alias = "samedi")]
  Sat,
  #[serde(alias = "dim", alias = "dimanche")]
  Sun,
}

impl Day {
  pub const ALL: [Day; 7] = [Day::Mon, Day::Tue, Day::Wed, Day::Thu, Day::Fri, Day::Sat, Day::Sun];

  pub fn index(self) -> usize {
    self as usize
  }

  pub fn from_index(i: usize) -> Option<Day> {
    Day::ALL.get(i).copied()
  }

  pub fn previous(self) -> Option<Day> {
    self.index().checked_sub(1).and_then(Day::from_index)
  }

  pub fn next(self) -> Option<Day> {
    Day::from_index(self.index() + 1)
  }

  pub fn key(self) -> &'static str {
    ["mon", "tue", "wed", "thu", "fri", "sat", "sun"][self.index()]
  }

  pub fn label(self) -> &'static str {
    ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"][self.index()]
  }

  pub fn short(self) -> &'static str {
    ["lun", "mar", "mer", "jeu", "ven", "sam", "dim"][self.index()]
  }

  /// Accepte `mon`, `lun`, `lundi`, sans casse.
  pub fn parse(s: &str) -> Option<Day> {
    let s = fold(s.trim());
    Day::ALL.into_iter().find(|d| s == d.key() || s == d.short() || s == d.label())
  }
}

impl fmt::Display for Day {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(self.key())
  }
}

/// Liste de jours écrite `mon,tue` ou `lun,mar` ; vide = toute la semaine.
pub fn parse_days(s: Option<&str>) -> Result<Vec<Day>, String> {
  let Some(s) = s.filter(|s| !s.trim().is_empty()) else {
    return Ok(Day::ALL.to_vec());
  };
  let mut days: Vec<Day> = s
    .split([',', ' ', ';'])
    .filter(|p| !p.is_empty())
    .map(|p| Day::parse(p).ok_or_else(|| format!("jour inconnu : {p}")))
    .collect::<Result<_, _>>()?;
  days.sort();
  days.dedup();
  Ok(days)
}

// -- unités -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Unit {
  #[serde(rename = "g")]
  G,
  #[serde(rename = "ml")]
  Ml,
  #[serde(rename = "pce")]
  Pce,
  #[serde(rename = "gousse")]
  Gousse,
  #[serde(rename = "botte")]
  Botte,
  #[serde(rename = "tranche")]
  Tranche,
  #[serde(rename = "cs")]
  Cs,
  #[serde(rename = "cc")]
  Cc,
  #[serde(rename = "pincée")]
  Pincee,
}

impl Unit {
  pub fn code(self) -> &'static str {
    match self {
      Unit::G => "g",
      Unit::Ml => "ml",
      Unit::Pce => "pce",
      Unit::Gousse => "gousse",
      Unit::Botte => "botte",
      Unit::Tranche => "tranche",
      Unit::Cs => "cs",
      Unit::Cc => "cc",
      Unit::Pincee => "pincée",
    }
  }

  /// Reconnaît une unité écrite librement et renvoie le facteur vers l'unité
  /// de référence (`kg` → `g` × 1000, `dl` → `ml` × 100…).
  pub fn parse(s: &str) -> Option<(Unit, f64)> {
    let s = fold(s.trim().trim_end_matches('.'));
    let unit = match s.as_str() {
      "g" | "gr" | "gramme" | "grammes" => (Unit::G, 1.0),
      "kg" | "kilo" | "kilos" => (Unit::G, 1000.0),
      "ml" => (Unit::Ml, 1.0),
      "cl" => (Unit::Ml, 10.0),
      "dl" => (Unit::Ml, 100.0),
      "l" | "litre" | "litres" => (Unit::Ml, 1000.0),
      "pce" | "pces" | "piece" | "pieces" | "pc" | "pcs" | "x" | "u" | "unite" | "unites" => (Unit::Pce, 1.0),
      "gousse" | "gousses" => (Unit::Gousse, 1.0),
      "botte" | "bottes" | "bouquet" | "bouquets" | "pot" => (Unit::Botte, 1.0),
      "tranche" | "tranches" => (Unit::Tranche, 1.0),
      "cs" | "cas" | "c.s" | "c. a soupe" | "cuillere a soupe" | "cuilleres a soupe" | "tbsp" => (Unit::Cs, 1.0),
      "cc" | "cac" | "c.c" | "c. a cafe" | "cuillere a cafe" | "cuilleres a cafe" | "tsp" => (Unit::Cc, 1.0),
      "pincee" | "pincees" => (Unit::Pincee, 1.0),
      _ => return None,
    };
    Some(unit)
  }
}

impl fmt::Display for Unit {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(self.code())
  }
}

// -- catalogue d'ingrédients -------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ingredient {
  /// Identifiant de rayon (voir `Settings::aisles`).
  pub aisle: String,
  /// Unité de référence : les recettes doivent l'employer pour que les
  /// quantités s'additionnent.
  pub unit: Unit,
  /// Fond de placard (huile, épices…) : listé à part, « à vérifier ».
  #[serde(default, skip_serializing_if = "std::ops::Not::not")]
  pub pantry: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
  pub items: BTreeMap<String, Ingredient>,
}

impl Catalog {
  /// Retrouve un ingrédient sans tenir compte de la casse, des accents ni d'un
  /// pluriel en -s/-x. Renvoie le nom canonique.
  pub fn resolve(&self, name: &str) -> Option<(&str, &Ingredient)> {
    let wanted = fold(name.trim());
    let exact = self.items.iter().find(|(key, _)| fold(key) == wanted);
    // Sinon, chaque mot au singulier des deux côtés : « pommes de terre » = « pomme de terre ».
    let loose = || {
      let wanted = singular(&wanted);
      self.items.iter().find(|(key, _)| singular(&fold(key)) == wanted)
    };
    exact.or_else(loose).map(|(key, ing)| (key.as_str(), ing))
  }
}

/// Retire le -s ou -x final des mots de plus de trois lettres.
fn singular(folded: &str) -> String {
  folded
    .split(' ')
    .map(|w| if w.chars().count() > 3 { w.trim_end_matches(['s', 'x']) } else { w })
    .collect::<Vec<_>>()
    .join(" ")
}

// -- recettes -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Meal {
  #[serde(alias = "souper", alias = "diner", alias = "dîner", alias = "supper")]
  Dinner,
  #[serde(alias = "petit-dejeuner", alias = "petit-déjeuner", alias = "breakfast")]
  Brunch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrunchKind {
  #[serde(alias = "sucre", alias = "sucré")]
  Sweet,
  #[serde(alias = "sale", alias = "salé")]
  Savory,
  Base,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protein {
  #[serde(alias = "poisson")]
  Fish,
  #[serde(alias = "volaille", alias = "poulet")]
  Poultry,
  #[serde(alias = "viande")]
  Meat,
  #[serde(alias = "vege", alias = "végé", alias = "vegetarien", alias = "végétarien", alias = "vegetarian")]
  Veggie,
  #[serde(alias = "legumineuses", alias = "légumineuses", alias = "legume")]
  Legumes,
}

impl Protein {
  pub fn label(self) -> &'static str {
    match self {
      Protein::Fish => "poisson",
      Protein::Poultry => "volaille",
      Protein::Meat => "viande",
      Protein::Veggie => "végétarien",
      Protein::Legumes => "légumineuses",
    }
  }

  pub fn is_vegetarian(self) -> bool {
    matches!(self, Protein::Veggie | Protein::Legumes)
  }
}

/// Une ligne d'ingrédient, quantité pour UN adulte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
  pub name: String,
  pub qty: f64,
  pub unit: Unit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
  pub id: String,
  pub name: String,
  pub meal: Meal,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub brunch_kind: Option<BrunchKind>,
  /// Durée totale, attentes comprises.
  pub minutes: u32,
  /// Temps passé à cuisiner pour de vrai.
  pub active_minutes: u32,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub protein: Option<Protein>,
  /// Féculent principal (`rice`, `pasta`, `potato`…), pour varier.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub starch: Option<String>,
  /// Se réchauffe bien : peut fournir les restes du lendemain.
  #[serde(default)]
  pub keeps: bool,
  /// Mois de saison ; vide = toute l'année.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub months: Vec<u32>,
  pub ingredients: Vec<Line>,
  #[serde(default)]
  pub steps: Vec<String>,
  #[serde(default)]
  pub favorite: bool,
  /// Écartée des propositions (« on n'aime pas »).
  #[serde(default)]
  pub excluded: bool,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub notes: Option<String>,
  /// `base`, `claude` ou `manual`.
  #[serde(default)]
  pub source: String,
  #[serde(default)]
  pub created_at: String,
  #[serde(default)]
  pub updated_at: String,
}

impl Recipe {
  pub fn in_season(&self, month: u32) -> bool {
    self.months.is_empty() || self.months.contains(&month)
  }

  pub fn is_quick(&self, limit: u32) -> bool {
    self.minutes <= limit
  }
}

// -- réglages ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Aisle {
  pub id: String,
  pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
  pub adults: u32,
  pub children: u32,
  /// Part d'une portion adulte que mange un enfant.
  pub child_factor: f64,
  /// Soirs où l'on peut cuisiner plus longtemps.
  pub long_days: Vec<Day>,
  /// Soirs où l'on mange les restes de la veille (le plat de la veille est
  /// alors préparé en double).
  pub leftover_days: Vec<Day>,
  /// Soirs sans souper planifié (dehors, invités…).
  pub free_days: Vec<Day>,
  pub brunch_days: Vec<Day>,
  /// Ajoute la recette « base du brunch » (pain, beurre, jus…) à chaque brunch.
  pub brunch_base: bool,
  /// Au-delà, un plat n'est pas proposé un soir de semaine ordinaire.
  pub quick_minutes: u32,
  /// Un plat n'est pas reproposé avant ce nombre de semaines.
  pub avoid_weeks: u32,
  /// Rayons, dans l'ordre du parcours en magasin.
  pub aisles: Vec<Aisle>,
  pub store_name: String,
}

impl Default for Settings {
  fn default() -> Self {
    let aisle = |id: &str, name: &str| Aisle {
      id: id.into(),
      name: name.into(),
    };
    Settings {
      adults: 2,
      children: 1,
      child_factor: 0.6,
      long_days: vec![Day::Sun],
      leftover_days: vec![Day::Thu],
      free_days: vec![],
      brunch_days: vec![Day::Sat, Day::Sun],
      brunch_base: true,
      quick_minutes: 35,
      avoid_weeks: 2,
      aisles: vec![
        aisle("fl", "Fruits & légumes"),
        aisle("bo", "Boulangerie"),
        aisle("vi", "Viande & volaille"),
        aisle("po", "Poisson"),
        aisle("ch", "Frais : charcuterie, traiteur, pâtes fraîches"),
        aisle("la", "Lait, crème, beurre & œufs"),
        aisle("fr", "Fromage"),
        aisle("pa", "Pâtes, riz & céréales"),
        aisle("co", "Conserves & sauces"),
        aisle("in", "International"),
        aisle("ep", "Huiles, épices & condiments"),
        aisle("pd", "Petit-déjeuner, sucre & farine"),
        aisle("su", "Surgelés"),
        aisle("bs", "Boissons"),
        aisle("di", "Divers"),
      ],
      store_name: "Coop".into(),
    }
  }
}

impl Settings {
  /// Nombre de portions adultes à préparer.
  pub fn portions(&self) -> f64 {
    self.adults as f64 + self.children as f64 * self.child_factor
  }

  pub fn aisle_name(&self, id: &str) -> String {
    self
      .aisles
      .iter()
      .find(|a| a.id == id)
      .map(|a| a.name.clone())
      .unwrap_or_else(|| "Divers".into())
  }

  pub fn has_aisle(&self, id: &str) -> bool {
    self.aisles.iter().any(|a| a.id == id)
  }

  pub fn validate(&self) -> Result<(), String> {
    if self.adults + self.children == 0 {
      return Err("il faut au moins une personne à table".into());
    }
    if !(0.0..=1.5).contains(&self.child_factor) {
      return Err("le facteur enfant doit être entre 0 et 1,5".into());
    }
    if self.aisles.is_empty() {
      return Err("il faut au moins un rayon".into());
    }
    let mut ids = BTreeSet::new();
    for aisle in &self.aisles {
      if aisle.id.trim().is_empty() || !ids.insert(&aisle.id) {
        return Err(format!("identifiant de rayon vide ou en double : « {} »", aisle.id));
      }
    }
    Ok(())
  }
}

// -- semaines ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Slot {
  #[default]
  Empty,
  Recipe {
    recipe: String,
  },
  /// Restes du dernier plat cuisiné avant ce soir.
  Leftovers,
  /// Pas de souper planifié.
  Free,
}

impl Slot {
  pub fn recipe(id: impl Into<String>) -> Self {
    Slot::Recipe { recipe: id.into() }
  }

  pub fn recipe_id(&self) -> Option<&str> {
    match self {
      Slot::Recipe { recipe } => Some(recipe),
      _ => None,
    }
  }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Extra {
  pub id: String,
  pub text: String,
  #[serde(default)]
  pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Week {
  pub week: WeekId,
  #[serde(default)]
  pub dinners: BTreeMap<Day, Slot>,
  #[serde(default)]
  pub brunch: BTreeMap<Day, Vec<String>>,
  /// Clés des articles cochés dans la liste de courses.
  #[serde(default)]
  pub checked: BTreeSet<String>,
  /// Articles ajoutés à la main (lait, papier toilette…).
  #[serde(default)]
  pub extras: Vec<Extra>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub note: Option<String>,
  #[serde(default)]
  pub updated_at: String,
}

impl Week {
  pub fn empty(week: WeekId) -> Self {
    Week {
      week,
      dinners: BTreeMap::new(),
      brunch: BTreeMap::new(),
      checked: BTreeSet::new(),
      extras: vec![],
      note: None,
      updated_at: String::new(),
    }
  }

  pub fn dinner(&self, day: Day) -> &Slot {
    static EMPTY: Slot = Slot::Empty;
    self.dinners.get(&day).unwrap_or(&EMPTY)
  }

  pub fn set_dinner(&mut self, day: Day, slot: Slot) {
    if slot == Slot::Empty {
      self.dinners.remove(&day);
    } else {
      self.dinners.insert(day, slot);
    }
  }

  pub fn set_brunch(&mut self, day: Day, recipes: Vec<String>) {
    if recipes.is_empty() {
      self.brunch.remove(&day);
    } else {
      self.brunch.insert(day, recipes);
    }
  }

  /// Toutes les recettes référencées, soupers et brunchs.
  pub fn recipe_ids(&self) -> impl Iterator<Item = &str> {
    self
      .dinners
      .values()
      .filter_map(Slot::recipe_id)
      .chain(self.brunch.values().flatten().map(String::as_str))
  }

  pub fn is_blank(&self) -> bool {
    self.dinners.is_empty() && self.brunch.is_empty() && self.extras.is_empty() && self.checked.is_empty() && self.note.is_none()
  }

  /// Remplace un identifiant de recette partout (renommage, suppression).
  pub fn replace_recipe(&mut self, from: &str, to: Option<&str>) {
    for slot in self.dinners.values_mut() {
      if slot.recipe_id() == Some(from) {
        *slot = to.map(Slot::recipe).unwrap_or(Slot::Empty);
      }
    }
    self.dinners.retain(|_, s| *s != Slot::Empty);
    for list in self.brunch.values_mut() {
      list.retain(|id| id != from);
      if let Some(to) = to {
        list.push(to.to_string());
      }
    }
    self.brunch.retain(|_, l| !l.is_empty());
  }
}

#[cfg(test)]
mod tests {
  #[test]
  fn resolves_plurals_word_by_word() {
    let catalog = crate::seed::catalog().unwrap();
    let name = |n: &str| catalog.resolve(n).map(|(k, _)| k.to_string());
    assert_eq!(name("Pommes de terre").as_deref(), Some("pomme de terre"));
    assert_eq!(name("oignons").as_deref(), Some("oignon"));
    assert_eq!(name("tomate cerise").as_deref(), Some("tomates cerises"));
    assert_eq!(name("Oeufs").as_deref(), Some("œufs"));
    assert_eq!(name("riz basmati").as_deref(), Some("riz basmati"));
    assert_eq!(name("menthe"), None);
  }
}
