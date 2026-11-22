//! Surveillance du dossier de données.
//!
//! Une modification faite hors de l'API (édition manuelle d'un JSON, `git
//! checkout`, restauration d'un instantané) est diffusée aux navigateurs
//! connectés. Les écritures du serveur lui-même sont ignorées : elles ont déjà
//! émis leur propre évènement.

use crate::app::AppState;
use crate::store::Change;
use anyhow::Result;
use notify::RecursiveMode;
use notify_debouncer_full::new_debouncer;
use std::time::Duration;

/// Fenêtre pendant laquelle un évènement fichier est considéré comme l'écho
/// d'une écriture du serveur.
const SELF_WRITE_WINDOW_MS: i64 = 1500;

/// Démarre la surveillance. La valeur retournée doit rester en vie.
pub fn spawn(state: AppState) -> Result<Box<dyn std::any::Any + Send>> {
  let root = state.store.root().to_path_buf();
  let notifier = state.clone();

  let mut debouncer = new_debouncer(
    Duration::from_millis(300),
    None,
    move |result: notify_debouncer_full::DebounceEventResult| match result {
      Ok(events) => {
        let relevant = events.iter().flat_map(|event| event.paths.iter()).any(|path| {
          let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
          name.ends_with(".json") && !name.ends_with(".json.tmp")
        });
        if !relevant || notifier.wrote_recently(SELF_WRITE_WINDOW_MS) {
          return;
        }
        tracing::debug!("modification externe détectée");
        notifier.store.notify(Change::scope("all"));
      }
      Err(errors) => tracing::warn!("surveillance du dossier : {errors:?}"),
    },
  )?;

  debouncer.watch(&root, RecursiveMode::Recursive)?;
  tracing::info!("surveillance de {}", root.display());
  Ok(Box::new(debouncer))
}
