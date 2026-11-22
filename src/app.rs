//! État partagé du serveur.

use crate::store::{Change, Store};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
  pub store: Store,
  /// Sérialise les écritures : deux téléphones qui cochent en même temps ne
  /// doivent pas s'écraser l'un l'autre (lecture-modification-écriture).
  pub write_lock: Arc<tokio::sync::Mutex<()>>,
  /// Jeton d'accès facultatif (`MENUS_TOKEN`).
  pub auth_token: Option<String>,
  /// Horodatage de la dernière écriture faite par le serveur lui-même, pour que
  /// la surveillance du dossier ne rediffuse pas nos propres modifications.
  last_self_write: Arc<AtomicI64>,
}

impl AppState {
  pub fn new(store: Store, auth_token: Option<String>) -> Self {
    AppState {
      store,
      write_lock: Arc::new(tokio::sync::Mutex::new(())),
      auth_token,
      last_self_write: Arc::new(AtomicI64::new(0)),
    }
  }

  /// À appeler après chaque écriture : marque l'écho et prévient les navigateurs.
  pub fn wrote(&self, change: Change) {
    self.last_self_write.store(now_millis(), Ordering::Relaxed);
    self.store.notify(change);
  }

  /// Vrai si une écriture du serveur date de moins de `window_ms`.
  pub fn wrote_recently(&self, window_ms: i64) -> bool {
    now_millis() - self.last_self_write.load(Ordering::Relaxed) < window_ms
  }
}

fn now_millis() -> i64 {
  chrono::Utc::now().timestamp_millis()
}
