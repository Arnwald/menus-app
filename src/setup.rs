//! `menus setup-claude` : déclare `menus mcp` dans `claude_desktop_config.json`, sans
//! toucher aux autres serveurs. L'ancien fichier est gardé à côté (`.bak`).

use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// Emplacement de la configuration de Claude Desktop sur ce système.
fn default_config() -> Result<PathBuf> {
  let home = || std::env::var_os("HOME").map(PathBuf::from).context("variable HOME absente");
  if cfg!(target_os = "macos") {
    return Ok(home()?.join("Library/Application Support/Claude/claude_desktop_config.json"));
  }
  if cfg!(windows) {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from).context("variable APPDATA absente")?;
    return Ok(appdata.join("Claude").join("claude_desktop_config.json"));
  }
  let base = std::env::var_os("XDG_CONFIG_HOME")
    .map(PathBuf::from)
    .map(Ok)
    .unwrap_or_else(|| home().map(|h| h.join(".config")))?;
  Ok(base.join("Claude").join("claude_desktop_config.json"))
}

/// L'entrée `menus` : ce binaire, en mode MCP, vers `url`.
fn entry(command: &Path, url: &str, token: Option<&str>) -> Value {
  let mut entry = json!({ "command": command.display().to_string(), "args": ["mcp", "--url", url] });
  if let Some(token) = token {
    entry["env"] = json!({ "MENUS_TOKEN": token });
  }
  entry
}

/// Ajoute ou remplace `mcpServers.menus` dans `config`.
fn merge(config: &mut Value, server: Value) -> Result<bool> {
  let Some(root) = config.as_object_mut() else {
    bail!("la configuration n'est pas un objet JSON");
  };
  let servers = root.entry("mcpServers").or_insert_with(|| Value::Object(Map::new()));
  let Some(servers) = servers.as_object_mut() else {
    bail!("« mcpServers » n'est pas un objet JSON");
  };
  Ok(servers.insert("menus".into(), server).is_some())
}

pub fn claude_desktop(url: &str, token: Option<String>, config: Option<PathBuf>) -> Result<()> {
  let path = match config {
    Some(path) => path,
    None => default_config()?,
  };
  let exe = std::env::current_exe().context("chemin de ce programme introuvable")?;
  let exe = exe.canonicalize().unwrap_or(exe);

  let mut current: Value = match std::fs::read_to_string(&path) {
    Ok(text) if !text.trim().is_empty() => serde_json::from_str(&text).with_context(|| format!("{} n'est pas du JSON valide", path.display()))?,
    Ok(_) => json!({}),
    Err(err) if err.kind() == std::io::ErrorKind::NotFound => json!({}),
    Err(err) => return Err(err).with_context(|| format!("lecture de {}", path.display())),
  };

  let replaced = merge(&mut current, entry(&exe, url, token.as_deref()))?;
  if let Some(parent) = path.parent() {
    std::fs::create_dir_all(parent).with_context(|| format!("création de {}", parent.display()))?;
  }
  if path.exists() {
    let backup = path.with_extension("json.bak");
    std::fs::copy(&path, &backup).with_context(|| format!("sauvegarde vers {}", backup.display()))?;
  }
  crate::store::write_json(&path, &current)?;

  println!(
    "{} le serveur MCP « menus » dans {}",
    if replaced { "Mis à jour :" } else { "Ajouté :" },
    path.display()
  );
  println!("  commande : {} mcp --url {url}", exe.display());
  if token.is_some() {
    println!("  jeton    : transmis par MENUS_TOKEN");
  }
  println!("Quitte et relance Claude Desktop (Cmd+Q) pour qu'il le charge ; `menus serve` doit tourner.");
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn keeps_other_servers_and_their_order() {
    let mut config = json!({ "globalShortcut": "", "mcpServers": { "LinFIR": { "command": "/Applications/LinFIR.app", "args": ["--mcp"] } } });
    let replaced = merge(&mut config, entry(Path::new("/u/.cargo/bin/menus"), "http://localhost:8080", None)).unwrap();
    assert!(!replaced);
    let servers = config["mcpServers"].as_object().unwrap();
    assert_eq!(servers.keys().collect::<Vec<_>>(), ["LinFIR", "menus"]);
    assert_eq!(servers["menus"]["args"], json!(["mcp", "--url", "http://localhost:8080"]));
    assert!(servers["menus"].get("env").is_none());

    let replaced = merge(&mut config, entry(Path::new("/u/.cargo/bin/menus"), "http://nas:8098", Some("abc"))).unwrap();
    assert!(replaced);
    assert_eq!(config["mcpServers"]["menus"]["env"]["MENUS_TOKEN"], "abc");
    assert_eq!(config["globalShortcut"], "");
  }

  #[test]
  fn writes_a_fresh_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Claude").join("claude_desktop_config.json");
    claude_desktop("http://localhost:8080", None, Some(path.clone())).unwrap();
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written["mcpServers"]["menus"]["args"][0], "mcp");
    // Second passage : une sauvegarde est faite, l'entrée remplacée.
    claude_desktop("http://nas:8098", Some("t".into()), Some(path.clone())).unwrap();
    assert!(path.with_extension("json.bak").exists());
  }
}
