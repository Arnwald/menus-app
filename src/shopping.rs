//! Liste de courses : additionne les ingrédients des repas d'une semaine,
//! arrondit à des quantités qu'on peut acheter, range par rayon.

use crate::model::{Catalog, Day, Extra, Recipe, Settings, Slot, Unit, Week};
use crate::slug::slugify;
use serde::Serialize;
use std::collections::BTreeMap;

/// Une préparation : la recette `recipe`, cuisinée le `day`, `times` fois
/// (2 quand le lendemain mange les restes).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cooking {
  pub day: Day,
  pub recipe: String,
  pub times: f64,
  pub brunch: bool,
}

/// Ce qu'il faut cuisiner, jour par jour. Un soir « restes » double le dernier
/// plat cuisiné avant lui dans la semaine.
pub fn cooking_plan(week: &Week) -> Vec<Cooking> {
  let mut plan: Vec<Cooking> = vec![];
  let mut last_cooked: Option<usize> = None;
  for day in Day::ALL {
    if let Some(items) = week.brunch.get(&day) {
      for id in items {
        plan.push(Cooking {
          day,
          recipe: id.clone(),
          times: 1.0,
          brunch: true,
        });
      }
    }
    match week.dinner(day) {
      Slot::Recipe { recipe } => {
        plan.push(Cooking {
          day,
          recipe: recipe.clone(),
          times: 1.0,
          brunch: false,
        });
        last_cooked = Some(plan.len() - 1);
      }
      Slot::Leftovers => {
        if let Some(i) = last_cooked {
          plan[i].times += 1.0;
        }
      }
      Slot::Empty | Slot::Free => {}
    }
  }
  plan
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
  /// Identifiant stable pour cocher : nom + unité.
  pub key: String,
  pub name: String,
  pub qty: f64,
  pub unit: Unit,
  /// Quantité lisible : « 450 g », « 2,5 dl », « 3 ».
  pub amount: String,
  /// Recettes qui utilisent l'article.
  pub used_in: Vec<String>,
  pub checked: bool,
  /// Absent du catalogue : rangé dans « Divers ».
  #[serde(skip_serializing_if = "std::ops::Not::not")]
  pub unknown: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Section {
  pub aisle: String,
  pub name: String,
  pub items: Vec<Item>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShoppingList {
  pub week: String,
  pub label: String,
  pub days: Vec<Day>,
  pub portions: f64,
  pub sections: Vec<Section>,
  /// Fonds de placard : à vérifier avant de partir.
  pub pantry: Vec<Item>,
  pub extras: Vec<Extra>,
  pub total: usize,
  pub checked: usize,
  /// Recettes référencées mais introuvables.
  pub missing: Vec<String>,
}

pub fn build(week: &Week, recipes: &[Recipe], catalog: &Catalog, settings: &Settings, days: &[Day]) -> ShoppingList {
  let portions = settings.portions();
  let by_id: BTreeMap<&str, &Recipe> = recipes.iter().map(|r| (r.id.as_str(), r)).collect();

  // (nom, unité) → (quantité, recettes)
  let mut totals: BTreeMap<(String, Unit), (f64, Vec<String>)> = BTreeMap::new();
  let mut missing = vec![];
  for cooking in cooking_plan(week).into_iter().filter(|c| days.contains(&c.day)) {
    let Some(recipe) = by_id.get(cooking.recipe.as_str()) else {
      missing.push(cooking.recipe);
      continue;
    };
    for line in &recipe.ingredients {
      let entry = totals.entry((line.name.clone(), line.unit)).or_insert((0.0, vec![]));
      entry.0 += line.qty * portions * cooking.times;
      if !entry.1.contains(&recipe.name) {
        entry.1.push(recipe.name.clone());
      }
    }
  }

  let mut sections: BTreeMap<String, Vec<Item>> = BTreeMap::new();
  let mut pantry = vec![];
  for ((name, unit), (qty, used_in)) in totals {
    let qty = round_for_shopping(qty, unit);
    let key = format!("{}-{}", slugify(&name), slugify(unit.code()));
    let known = catalog.resolve(&name).map(|(_, ing)| ing.clone());
    let item = Item {
      checked: week.checked.contains(&key),
      key,
      amount: format_amount(qty, unit),
      name,
      qty,
      unit,
      used_in,
      unknown: known.is_none(),
    };
    match known {
      Some(ing) if ing.pantry => pantry.push(item),
      Some(ing) if settings.has_aisle(&ing.aisle) => sections.entry(ing.aisle).or_default().push(item),
      _ => sections.entry("di".into()).or_default().push(item),
    }
  }

  // Ordre du parcours en magasin ; un rayon inconnu des réglages finit en dernier.
  let mut ordered: Vec<Section> = settings
    .aisles
    .iter()
    .filter_map(|aisle| {
      sections.remove(&aisle.id).map(|items| Section {
        aisle: aisle.id.clone(),
        name: aisle.name.clone(),
        items,
      })
    })
    .collect();
  for (aisle, items) in sections {
    ordered.push(Section {
      name: settings.aisle_name(&aisle),
      aisle,
      items,
    });
  }
  for section in &mut ordered {
    section.items.sort_by_key(|i| crate::slug::fold(&i.name));
  }
  pantry.sort_by_key(|i| crate::slug::fold(&i.name));

  let total = ordered.iter().map(|s| s.items.len()).sum::<usize>() + week.extras.len();
  let checked = ordered.iter().flat_map(|s| &s.items).filter(|i| i.checked).count() + week.extras.iter().filter(|e| e.done).count();

  ShoppingList {
    week: week.week.to_string(),
    label: week.week.label(),
    days: days.to_vec(),
    portions,
    sections: ordered,
    pantry,
    extras: week.extras.clone(),
    total,
    checked,
    missing,
  }
}

/// Arrondit vers le haut à une quantité qu'on trouve en magasin.
pub fn round_for_shopping(qty: f64, unit: Unit) -> f64 {
  if qty <= 0.0 {
    return 0.0;
  }
  let up = |step: f64| (qty / step - 1e-9).ceil() * step;
  match unit {
    Unit::G => up(if qty < 50.0 {
      5.0
    } else if qty < 250.0 {
      10.0
    } else if qty < 1000.0 {
      50.0
    } else {
      100.0
    }),
    Unit::Ml => up(if qty < 100.0 {
      10.0
    } else if qty < 1000.0 {
      50.0
    } else {
      100.0
    }),
    Unit::Pce | Unit::Gousse | Unit::Botte | Unit::Tranche => {
      // 3,1 œufs → 3 ; 2,4 poivrons → 3.
      let floor = qty.floor();
      if floor >= 1.0 && qty - floor < 0.15 {
        floor
      } else {
        qty.ceil().max(1.0)
      }
    }
    Unit::Cs | Unit::Cc | Unit::Pincee => up(0.5),
  }
}

/// Quantité pour cuisiner : moins grossière que pour acheter.
pub fn round_for_cooking(qty: f64, unit: Unit) -> f64 {
  let near = |step: f64| ((qty / step).round() * step).max(step);
  match unit {
    Unit::G => near(if qty < 20.0 {
      1.0
    } else if qty < 200.0 {
      5.0
    } else {
      10.0
    }),
    Unit::Ml => near(if qty < 50.0 { 5.0 } else { 10.0 }),
    _ => near(0.25),
  }
}

/// Nombre à la française : 2,5 ; 3 ; 0,25.
pub fn number(x: f64) -> String {
  let rounded = (x * 100.0).round() / 100.0;
  let mut s = format!("{rounded:.2}");
  while s.ends_with('0') {
    s.pop();
  }
  if s.ends_with('.') {
    s.pop();
  }
  s.replace('.', ",")
}

/// « 450 g », « 1,2 kg », « 2,5 dl », « 3 », « 2 gousses ».
pub fn format_amount(qty: f64, unit: Unit) -> String {
  let plural = |one: &str, many: &str| format!("{} {}", number(qty), if qty > 1.0 { many } else { one });
  match unit {
    Unit::G if qty >= 1000.0 => format!("{} kg", number(qty / 1000.0)),
    Unit::G => format!("{} g", number(qty)),
    Unit::Ml if qty >= 1000.0 => format!("{} l", number(qty / 1000.0)),
    Unit::Ml if qty >= 100.0 => format!("{} dl", number(qty / 100.0)),
    Unit::Ml => format!("{} ml", number(qty)),
    Unit::Pce => number(qty),
    Unit::Gousse => plural("gousse", "gousses"),
    Unit::Botte => plural("botte", "bottes"),
    Unit::Tranche => plural("tranche", "tranches"),
    Unit::Cs => format!("{} c. à soupe", number(qty)),
    Unit::Cc => format!("{} c. à café", number(qty)),
    Unit::Pincee => plural("pincée", "pincées"),
  }
}

/// Quantité pour la fiche recette : fractions pour ce qui se compte
/// (« 1 ¼ avocat », « ¾ botte »), sinon comme pour les courses.
pub fn format_cooking(qty: f64, unit: Unit) -> String {
  let counted = matches!(unit, Unit::Pce | Unit::Gousse | Unit::Botte | Unit::Tranche);
  if !counted {
    return format_amount(round_for_cooking(qty, unit), unit);
  }
  let q = round_for_cooking(qty, unit);
  let whole = q.floor();
  let fraction = match ((q - whole) * 4.0).round() as u8 {
    1 => "¼",
    2 => "½",
    3 => "¾",
    _ => "",
  };
  let number = match (whole as u64, fraction) {
    (0, f) => f.to_string(),
    (w, "") => w.to_string(),
    (w, f) => format!("{w} {f}"),
  };
  let word = match unit {
    Unit::Gousse => Some(("gousse", "gousses")),
    Unit::Botte => Some(("botte", "bottes")),
    Unit::Tranche => Some(("tranche", "tranches")),
    _ => None,
  };
  match word {
    Some((one, many)) => format!("{number} {}", if q > 1.0 { many } else { one }),
    None => number,
  }
}

fn capitalize(s: &str) -> String {
  let mut chars = s.chars();
  chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Version texte, à coller dans une note ou un message.
pub fn to_text(list: &ShoppingList, settings: &Settings) -> String {
  let mut out = format!("Courses {} — semaine du {}\n", settings.store_name, list.label);
  if list.days.len() < 7 {
    let days: Vec<&str> = list.days.iter().map(|d| d.short()).collect();
    out.push_str(&format!("(repas du {} seulement)\n", days.join(", ")));
  }
  let line = |name: &str, amount: &str| {
    let name = capitalize(name);
    if amount.is_empty() {
      format!("☐ {name}\n")
    } else {
      format!("☐ {name} — {amount}\n")
    }
  };
  for section in &list.sections {
    let open: Vec<&Item> = section.items.iter().filter(|i| !i.checked).collect();
    if open.is_empty() {
      continue;
    }
    out.push_str(&format!("\n{}\n", section.name));
    for item in open {
      out.push_str(&line(&item.name, &item.amount));
    }
  }
  let extras: Vec<&Extra> = list.extras.iter().filter(|e| !e.done).collect();
  if !extras.is_empty() {
    out.push_str("\nAutres\n");
    for extra in extras {
      out.push_str(&line(&extra.text, ""));
    }
  }
  let pantry: Vec<&Item> = list.pantry.iter().filter(|i| !i.checked).collect();
  if !pantry.is_empty() {
    out.push_str("\nPlacard, à vérifier\n");
    for item in pantry {
      out.push_str(&line(&item.name, &item.amount));
    }
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::model::Slot;
  use crate::weekid::WeekId;

  fn fixture() -> (Vec<Recipe>, Catalog, Settings) {
    let settings = Settings::default();
    let catalog = crate::seed::catalog().unwrap();
    let recipes = crate::seed::recipes(&catalog, &settings).unwrap();
    (recipes, catalog, settings)
  }

  #[test]
  fn rounds_to_buyable_amounts() {
    assert_eq!(round_for_shopping(364.0, Unit::G), 400.0);
    assert_eq!(round_for_shopping(31.0, Unit::G), 35.0);
    assert_eq!(round_for_shopping(1234.0, Unit::G), 1300.0);
    assert_eq!(round_for_shopping(3.05, Unit::Pce), 3.0);
    assert_eq!(round_for_shopping(2.4, Unit::Pce), 3.0);
    assert_eq!(round_for_shopping(0.39, Unit::Botte), 1.0);
    assert_eq!(round_for_shopping(1.3, Unit::Cs), 1.5);
    assert_eq!(format_amount(250.0, Unit::Ml), "2,5 dl");
    assert_eq!(format_amount(1300.0, Unit::G), "1,3 kg");
    assert_eq!(format_amount(2.0, Unit::Gousse), "2 gousses");
    assert_eq!(format_cooking(1.3, Unit::Pce), "1 ¼");
    assert_eq!(format_cooking(0.78, Unit::Botte), "¾ botte");
    assert_eq!(format_cooking(2.6, Unit::Gousse), "2 ½ gousses");
    assert_eq!(format_cooking(364.0, Unit::G), "360 g");
  }

  #[test]
  fn leftovers_double_the_previous_dinner() {
    let mut week = Week::empty(WeekId::current());
    week.set_dinner(Day::Wed, Slot::recipe("chili-sin-carne"));
    week.set_dinner(Day::Thu, Slot::Leftovers);
    week.set_dinner(Day::Fri, Slot::Free);
    week.set_dinner(Day::Sat, Slot::Leftovers);
    let plan = cooking_plan(&week);
    assert_eq!(
      plan,
      vec![Cooking {
        day: Day::Wed,
        recipe: "chili-sin-carne".into(),
        times: 3.0,
        brunch: false
      }]
    );
  }

  #[test]
  fn aggregates_across_recipes_and_portions() {
    let (recipes, catalog, settings) = fixture();
    let mut week = Week::empty(WeekId::current());
    week.set_dinner(Day::Mon, Slot::recipe("saumon-brocoli-riz")); // brocoli 200 g
    week.set_dinner(Day::Tue, Slot::recipe("boeuf-brocoli")); // brocoli 150 g
    week.set_dinner(Day::Wed, Slot::recipe("chili-sin-carne"));
    week.set_dinner(Day::Thu, Slot::Leftovers);
    week.set_brunch(Day::Sat, vec!["base-brunch".into(), "shakshuka".into()]);
    week.checked.insert("riz-basmati-g".into());

    let list = build(&week, &recipes, &catalog, &settings, &Day::ALL);
    let portions = settings.portions(); // 2,6
    let find = |name: &str| list.sections.iter().flat_map(|s| &s.items).find(|i| i.name == name).cloned();

    let brocoli = find("brocoli").unwrap();
    assert_eq!(brocoli.qty, round_for_shopping(350.0 * portions, Unit::G));
    assert_eq!(brocoli.used_in.len(), 2);

    // Riz : saumon 75 + bœuf 70 + chili 60 × 2 (restes) = 265 g par adulte.
    let riz = find("riz basmati").unwrap();
    assert_eq!(riz.qty, round_for_shopping(265.0 * portions, Unit::G));
    assert!(riz.checked);
    assert_eq!(list.checked, 1);

    // Les épices partent au placard, pas dans les rayons.
    assert!(list.pantry.iter().any(|i| i.name == "cumin"));
    assert!(find("cumin").is_none());

    // Le premier rayon est celui des fruits & légumes.
    assert_eq!(list.sections[0].aisle, "fl");

    // Filtrer sur le samedi ne garde que le brunch.
    let saturday = build(&week, &recipes, &catalog, &settings, &[Day::Sat]);
    assert!(saturday.sections.iter().flat_map(|s| &s.items).any(|i| i.name == "tresse (Zopf)"));
    assert!(!saturday.sections.iter().flat_map(|s| &s.items).any(|i| i.name == "brocoli"));

    let text = to_text(&list, &settings);
    assert!(text.contains("Fruits & légumes"));
    assert!(!text.contains("Riz basmati"), "les articles cochés ne sont pas recopiés");
    assert!(text.contains("☐ Brocoli — "));
  }
}
