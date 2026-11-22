//! Comparaison de noms sans accents et identifiants lisibles.

/// Minuscules, sans accents, espaces normalisés : « Œufs  Brouillés » → « oeufs brouilles ».
pub fn fold(s: &str) -> String {
  let mut out = String::with_capacity(s.len());
  for c in s.chars().flat_map(char::to_lowercase) {
    match c {
      'à' | 'â' | 'ä' | 'á' | 'ã' => out.push('a'),
      'é' | 'è' | 'ê' | 'ë' => out.push('e'),
      'î' | 'ï' | 'í' | 'ì' => out.push('i'),
      'ô' | 'ö' | 'ó' | 'ò' | 'õ' => out.push('o'),
      'ù' | 'û' | 'ü' | 'ú' => out.push('u'),
      'ç' => out.push('c'),
      'ñ' => out.push('n'),
      'ÿ' => out.push('y'),
      'œ' => out.push_str("oe"),
      'æ' => out.push_str("ae"),
      '’' | '`' => out.push('\''),
      c if c.is_whitespace() => {
        if !out.ends_with(' ') && !out.is_empty() {
          out.push(' ');
        }
      }
      c => out.push(c),
    }
  }
  out.trim_end().to_string()
}

/// Identifiant ASCII : « Älplermagronen & compote » → « alplermagronen-compote ».
pub fn slugify(s: &str) -> String {
  let mut out = String::new();
  for c in fold(s).chars() {
    if c.is_ascii_alphanumeric() {
      out.push(c);
    } else if !out.ends_with('-') && !out.is_empty() {
      out.push('-');
    }
  }
  let out = out.trim_end_matches('-');
  let mut out: String = out.chars().take(48).collect();
  while out.ends_with('-') {
    out.pop();
  }
  if out.is_empty() {
    "recette".into()
  } else {
    out
  }
}

/// Premier identifiant libre : `base`, puis `base-2`, `base-3`…
pub fn unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
  if !taken(base) {
    return base.to_string();
  }
  (2..).map(|n| format!("{base}-{n}")).find(|candidate| !taken(candidate)).expect("suite infinie")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn folds_accents_and_spaces() {
    assert_eq!(fold("  Œufs   Brouillés "), "oeufs brouilles");
    assert_eq!(fold("Crème fraîche"), "creme fraiche");
  }

  #[test]
  fn slugs_are_ascii() {
    assert_eq!(slugify("Älplermagronen & compote de pommes"), "alplermagronen-compote-de-pommes");
    assert_eq!(slugify("!!!"), "recette");
  }

  #[test]
  fn unique_appends_counter() {
    let taken = ["a", "a-2"];
    assert_eq!(unique("a", |c| taken.contains(&c)), "a-3");
    assert_eq!(unique("b", |c| taken.contains(&c)), "b");
  }
}
