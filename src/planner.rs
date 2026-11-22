//! Proposition de semaine sans Claude : tirage pondéré sous contraintes.
//!
//! Règles, de la plus dure à la plus souple :
//! - pas deux fois la même recette dans la semaine, rien d'exclu, de saison ;
//! - soir ordinaire : plat rapide ; soir « long » : plat plus mijoté favorisé ;
//! - veille d'un soir « restes » : un plat qui se réchauffe bien ;
//! - pas de recette servie dans les `avoid_weeks` semaines voisines ;
//! - équilibre visé : 2 poissons, au moins 3 soirs végétariens dont 1 de
//!   légumineuses, au plus 2 viandes et 2 volailles ;
//! - féculents variés d'un soir à l'autre ; favoris plus fréquents.
//!
//! Quand aucune recette ne passe, les contraintes souples sont relâchées une à une.

use crate::model::{BrunchKind, Day, Meal, Protein, Recipe, Settings, Slot, Week};
use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::HashMap;

pub struct Planner<'a> {
  pub recipes: &'a [Recipe],
  pub settings: &'a Settings,
  /// Écart en semaines avec la semaine voisine la plus proche où la recette figure.
  pub history: &'a HashMap<String, u32>,
  pub month: u32,
}

/// Contraintes relâchables, dans l'ordre où on les abandonne.
#[derive(Clone, Copy)]
struct Strictness {
  recency: bool,
  /// Quand il reste juste assez de soirs pour atteindre l'équilibre, seules
  /// les catégories manquantes sont admises.
  quota: bool,
  quick: bool,
  keeps: bool,
}

const LEVELS: [Strictness; 5] = [
  Strictness {
    recency: true,
    quota: true,
    quick: true,
    keeps: true,
  },
  Strictness {
    recency: false,
    quota: true,
    quick: true,
    keeps: true,
  },
  Strictness {
    recency: false,
    quota: false,
    quick: true,
    keeps: true,
  },
  Strictness {
    recency: false,
    quota: false,
    quick: false,
    keeps: true,
  },
  Strictness {
    recency: false,
    quota: false,
    quick: false,
    keeps: false,
  },
];

/// Soupers de poisson et soirs végétariens visés par semaine.
const FISH_TARGET: usize = 2;
const VEGETARIAN_TARGET: usize = 3;

impl<'a> Planner<'a> {
  fn by_id(&self, id: &str) -> Option<&'a Recipe> {
    self.recipes.iter().find(|r| r.id == id)
  }

  /// Ce que doit être chaque soir d'après les réglages : libre, restes, ou à cuisiner.
  pub fn skeleton(&self) -> Vec<(Day, Slot)> {
    let s = self.settings;
    let mut out: Vec<(Day, Slot)> = vec![];
    for day in Day::ALL {
      let previous_cooks = out.last().is_some_and(|(_, slot)| *slot == Slot::Empty);
      let slot = if s.free_days.contains(&day) {
        Slot::Free
      } else if s.leftover_days.contains(&day) && previous_cooks {
        Slot::Leftovers
      } else {
        Slot::Empty
      };
      out.push((day, slot));
    }
    out
  }

  /// Propose les soupers et les brunchs. Avec `keep`, les soirs déjà remplis
  /// de `current` sont conservés et comptent dans l'équilibre.
  pub fn propose(&self, current: &Week, keep: bool, rng: &mut impl Rng) -> Week {
    let mut week = current.clone();
    if !keep {
      week.dinners.clear();
      week.brunch.clear();
    }

    for (day, slot) in self.skeleton() {
      if week.dinner(day) == &Slot::Empty && slot != Slot::Empty {
        week.set_dinner(day, slot);
      }
    }

    // Les soirs les plus contraints d'abord : veilles de restes, soirs longs.
    let mut to_fill: Vec<Day> = Day::ALL.into_iter().filter(|d| week.dinner(*d) == &Slot::Empty).collect();
    to_fill.shuffle(rng);
    to_fill.sort_by_key(|d| {
      let feeds_leftovers = self.feeds_leftovers(&week, *d);
      let long = self.settings.long_days.contains(d);
      (!feeds_leftovers, !long)
    });
    let count = to_fill.len();
    for (i, day) in to_fill.into_iter().enumerate() {
      if let Some(recipe) = self.pick_dinner(&week, day, None, count - i, rng) {
        week.set_dinner(day, Slot::recipe(recipe));
      }
    }

    for day in self.settings.brunch_days.clone() {
      if keep && week.brunch.contains_key(&day) {
        continue;
      }
      let items = self.pick_brunch(&week, day, rng);
      week.set_brunch(day, items);
    }
    week
  }

  /// Une autre recette pour un soir, le reste de la semaine fixé.
  pub fn reroll(&self, week: &Week, day: Day, rng: &mut impl Rng) -> Option<String> {
    let current = week.dinner(day).recipe_id().map(str::to_string);
    let mut others = week.clone();
    others.set_dinner(day, Slot::Empty);
    self.pick_dinner(&others, day, current.as_deref(), usize::MAX, rng)
  }

  /// Vrai si le soir suivant mange les restes de `day`.
  fn feeds_leftovers(&self, week: &Week, day: Day) -> bool {
    day.next().is_some_and(|next| week.dinner(next) == &Slot::Leftovers)
  }

  /// `remaining` : soirs encore à remplir, celui-ci compris.
  fn pick_dinner(&self, week: &Week, day: Day, avoid: Option<&str>, remaining: usize, rng: &mut impl Rng) -> Option<String> {
    let chosen: Vec<&Recipe> = week.dinners.values().filter_map(Slot::recipe_id).filter_map(|id| self.by_id(id)).collect();
    let neighbours: Vec<&Recipe> = [day.previous(), day.next()]
      .into_iter()
      .flatten()
      .filter_map(|d| week.dinner(d).recipe_id())
      .filter_map(|id| self.by_id(id))
      .collect();
    let needs_keeps = self.feeds_leftovers(week, day);
    let long = self.settings.long_days.contains(&day);
    let allowed = needed_proteins(&chosen, remaining);

    for strict in LEVELS {
      let weighted: Vec<(&Recipe, f64)> = self
        .recipes
        .iter()
        .filter(|r| r.meal == Meal::Dinner && Some(r.id.as_str()) != avoid)
        .filter(|r| !strict.quota || allowed.as_ref().is_none_or(|ok| ok.contains(&r.protein.unwrap_or(Protein::Veggie))))
        .map(|r| (r, self.dinner_weight(r, &chosen, &neighbours, needs_keeps, long, strict)))
        .filter(|(_, w)| *w > 0.0)
        .collect();
      if let Some(recipe) = draw(&weighted, rng) {
        return Some(recipe.id.clone());
      }
    }
    None
  }

  fn dinner_weight(&self, r: &Recipe, chosen: &[&Recipe], neighbours: &[&Recipe], needs_keeps: bool, long: bool, strict: Strictness) -> f64 {
    let s = self.settings;
    if r.excluded || !r.in_season(self.month) || chosen.iter().any(|c| c.id == r.id) {
      return 0.0;
    }
    let mut w = 1.0;

    let gap = self.history.get(&r.id).copied().unwrap_or(u32::MAX);
    if gap < s.avoid_weeks.max(1) {
      if strict.recency {
        return 0.0;
      }
      w *= 0.2;
    } else if gap <= s.avoid_weeks + 1 {
      w *= 0.5;
    }

    let quick = r.is_quick(s.quick_minutes);
    if long {
      // Le soir long sert à cuisiner autre chose que d'habitude.
      if !quick {
        w *= 4.0;
      }
    } else if !quick {
      if strict.quick {
        return 0.0;
      }
      w *= 0.2;
    }

    if needs_keeps && !r.keeps {
      if strict.keeps {
        return 0.0;
      }
      w *= 0.2;
    }

    let count = |p: Protein| chosen.iter().filter(|c| c.protein == Some(p)).count();
    let vegetarian = chosen.iter().filter(|c| c.protein.is_some_and(Protein::is_vegetarian)).count();
    // Plafonds : au-delà, exclu tant que l'équilibre est exigé, très rare sinon.
    let over = |limit: usize, p: Protein| count(p) >= limit;
    let capped = |limit: usize, p: Protein| over(limit, p) && strict.quota;
    if capped(FISH_TARGET, Protein::Fish) && r.protein == Some(Protein::Fish)
      || capped(2, Protein::Meat) && r.protein == Some(Protein::Meat)
      || capped(2, Protein::Poultry) && r.protein == Some(Protein::Poultry)
    {
      return 0.0;
    }
    match r.protein.unwrap_or(Protein::Veggie) {
      Protein::Fish => w *= if over(FISH_TARGET, Protein::Fish) { 0.1 } else { 2.0 },
      Protein::Meat => {
        w *= match count(Protein::Meat) {
          0 => 1.0,
          1 => 0.4,
          _ => 0.03,
        }
      }
      Protein::Poultry => w *= if over(2, Protein::Poultry) { 0.1 } else { 1.0 },
      Protein::Legumes => {
        if count(Protein::Legumes) == 0 {
          w *= 2.0;
        }
        if vegetarian < VEGETARIAN_TARGET {
          w *= 1.4;
        }
      }
      Protein::Veggie => {
        if vegetarian < VEGETARIAN_TARGET {
          w *= 1.4;
        }
      }
    }

    if let Some(starch) = &r.starch {
      if neighbours.iter().any(|n| n.starch.as_ref() == Some(starch)) {
        w *= 0.25;
      }
      match chosen.iter().filter(|c| c.starch.as_ref() == Some(starch)).count() {
        0 | 1 => {}
        2 => w *= 0.4,
        _ => w *= 0.1,
      }
    }

    if r.favorite {
      w *= 2.0;
    }
    w
  }

  /// Base du brunch (si activée) + un sucré + un salé, différents de l'autre jour.
  fn pick_brunch(&self, week: &Week, day: Day, rng: &mut impl Rng) -> Vec<String> {
    let taken: Vec<&str> = week
      .brunch
      .iter()
      .filter(|(d, _)| **d != day)
      .flat_map(|(_, ids)| ids.iter().map(String::as_str))
      .collect();
    let mut out = vec![];
    if self.settings.brunch_base {
      if let Some(base) = self
        .recipes
        .iter()
        .find(|r| r.meal == Meal::Brunch && r.brunch_kind == Some(BrunchKind::Base) && !r.excluded)
      {
        out.push(base.id.clone());
      }
    }
    for kind in [BrunchKind::Sweet, BrunchKind::Savory] {
      let weighted: Vec<(&Recipe, f64)> = self
        .recipes
        .iter()
        .filter(|r| r.meal == Meal::Brunch && r.brunch_kind == Some(kind) && !r.excluded && r.in_season(self.month))
        .filter(|r| !taken.contains(&r.id.as_str()))
        .map(|r| {
          let gap = self.history.get(&r.id).copied().unwrap_or(u32::MAX);
          let mut w = if gap < 1 {
            0.3
          } else if gap < 2 {
            0.6
          } else {
            1.0
          };
          if r.favorite {
            w *= 2.0;
          }
          (r, w)
        })
        .collect();
      if let Some(r) = draw(&weighted, rng) {
        out.push(r.id.clone());
      }
    }
    out
  }
}

/// Catégories encore admises quand les soirs restants suffisent tout juste à
/// atteindre l'équilibre ; `None` = toutes.
fn needed_proteins(chosen: &[&Recipe], remaining: usize) -> Option<Vec<Protein>> {
  let count = |f: &dyn Fn(Protein) -> bool| chosen.iter().filter(|c| f(c.protein.unwrap_or(Protein::Veggie))).count();
  let fish = FISH_TARGET.saturating_sub(count(&|p| p == Protein::Fish));
  let vegetarian = VEGETARIAN_TARGET.saturating_sub(count(&Protein::is_vegetarian));
  let legumes = 1usize.saturating_sub(count(&|p| p == Protein::Legumes));
  if fish + vegetarian.max(legumes) < remaining {
    return None;
  }
  let mut allowed = vec![];
  if fish > 0 {
    allowed.push(Protein::Fish);
  }
  if legumes > 0 {
    allowed.push(Protein::Legumes);
  }
  if vegetarian > legumes {
    allowed.push(Protein::Veggie);
  }
  Some(allowed)
}

fn draw<'r>(weighted: &[(&'r Recipe, f64)], rng: &mut impl Rng) -> Option<&'r Recipe> {
  let total: f64 = weighted.iter().map(|(_, w)| w).sum();
  if total <= 0.0 {
    return None;
  }
  let mut x = rng.gen_range(0.0..total);
  for (recipe, w) in weighted {
    if x < *w {
      return Some(recipe);
    }
    x -= w;
  }
  weighted.last().map(|(r, _)| *r)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::weekid::WeekId;
  use rand::rngs::StdRng;
  use rand::SeedableRng;

  #[test]
  fn proposals_respect_the_rules() {
    let settings = Settings::default();
    let catalog = crate::seed::catalog().unwrap();
    let recipes = crate::seed::recipes(&catalog, &settings).unwrap();
    let history = HashMap::from([("saumon-brocoli-riz".to_string(), 1u32)]);
    let planner = Planner {
      recipes: &recipes,
      settings: &settings,
      history: &history,
      month: 11,
    };
    let get = |id: &str| recipes.iter().find(|r| r.id == id).unwrap();
    let week = Week::empty("2026-W47".parse::<WeekId>().unwrap());

    for seed in 0..1000 {
      let mut rng = StdRng::seed_from_u64(seed);
      let plan = planner.propose(&week, false, &mut rng);

      // Jeudi = restes de mercredi, qui doit donc bien se garder.
      assert_eq!(plan.dinner(Day::Thu), &Slot::Leftovers);
      let wednesday = get(plan.dinner(Day::Wed).recipe_id().expect("mercredi rempli"));
      assert!(wednesday.keeps, "graine {seed} : {} ne se garde pas", wednesday.id);

      let dinners: Vec<&Recipe> = plan.dinners.values().filter_map(Slot::recipe_id).map(get).collect();
      assert_eq!(dinners.len(), 6, "graine {seed}");
      let mut ids: Vec<&str> = dinners.iter().map(|r| r.id.as_str()).collect();
      ids.sort();
      ids.dedup();
      assert_eq!(ids.len(), 6, "graine {seed} : doublon");
      assert!(!ids.contains(&"saumon-brocoli-riz"), "servi la semaine dernière");

      for day in [Day::Mon, Day::Tue, Day::Wed, Day::Fri, Day::Sat] {
        let r = get(plan.dinner(day).recipe_id().unwrap());
        assert!(r.minutes <= settings.quick_minutes, "graine {seed} : {} trop long un {day}", r.id);
        assert!(r.in_season(11), "{} hors saison", r.id);
      }

      let count = |p: Protein| dinners.iter().filter(|r| r.protein == Some(p)).count();
      assert_eq!(count(Protein::Fish), 2, "graine {seed} : poisson");
      assert!(
        dinners.iter().filter(|r| r.protein.is_some_and(Protein::is_vegetarian)).count() >= 3,
        "graine {seed} : pas assez de soirs végétariens"
      );
      assert!(count(Protein::Legumes) >= 1, "graine {seed} : pas de légumineuses");
      assert!(count(Protein::Meat) <= 2, "graine {seed} : trop de viande");

      let saturday = plan.brunch.get(&Day::Sat).expect("brunch samedi");
      let sunday = plan.brunch.get(&Day::Sun).expect("brunch dimanche");
      assert_eq!(saturday.len(), 3);
      assert_eq!(saturday[0], "base-brunch");
      assert!(saturday[1..].iter().all(|id| !sunday.contains(id)), "même brunch deux jours");
    }
  }

  #[test]
  fn keeps_filled_days_and_rerolls_one() {
    let settings = Settings::default();
    let catalog = crate::seed::catalog().unwrap();
    let recipes = crate::seed::recipes(&catalog, &settings).unwrap();
    let history = HashMap::new();
    let planner = Planner {
      recipes: &recipes,
      settings: &settings,
      history: &history,
      month: 11,
    };
    let mut week = Week::empty(WeekId::current());
    week.set_dinner(Day::Mon, Slot::recipe("raclette"));
    let mut rng = StdRng::seed_from_u64(7);

    let plan = planner.propose(&week, true, &mut rng);
    assert_eq!(plan.dinner(Day::Mon), &Slot::recipe("raclette"));

    let other = planner.reroll(&plan, Day::Tue, &mut rng).unwrap();
    assert_ne!(Some(other.as_str()), plan.dinner(Day::Tue).recipe_id());
    assert!(plan
      .dinners
      .values()
      .filter_map(Slot::recipe_id)
      .all(|id| id != other || id == plan.dinner(Day::Tue).recipe_id().unwrap()));
  }
}
