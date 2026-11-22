mod api;
mod app;
mod mcp;
mod model;
mod planner;
mod recipes;
mod seed;
mod setup;
mod shopping;
mod slug;
mod store;
mod watch;
mod web;
mod weekid;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rand::SeedableRng;
use std::path::PathBuf;
use weekid::WeekId;

#[derive(Parser)]
#[command(name = "menus", version, about = "Menus de la semaine et liste de courses : base JSON en clair, API, UI web")]
struct Cli {
  /// Dossier de la base (défaut : ./data, ou $MENUS_DATA_DIR).
  #[arg(long, global = true, env = "MENUS_DATA_DIR", default_value = "data")]
  data: PathBuf,

  #[command(subcommand)]
  command: Command,
}

#[derive(Subcommand)]
enum Command {
  /// Démarre le serveur web et l'API.
  Serve {
    #[arg(long, env = "MENUS_PORT", default_value_t = 8080)]
    port: u16,
    #[arg(long, env = "MENUS_HOST", default_value = "0.0.0.0")]
    host: String,
  },
  /// Affiche la liste de courses d'une semaine.
  List {
    /// Semaine : 2026-W47, une date, « courante » ou « prochaine » (défaut : courante).
    #[arg(long, short, default_value = "courante")]
    week: String,
    /// Jours à couvrir, par exemple « lun,mar,mer » (défaut : toute la semaine).
    #[arg(long, short)]
    days: Option<String>,
  },
  /// Propose les soupers et brunchs d'une semaine (sans Claude) et l'enregistre.
  Propose {
    #[arg(long, short, default_value = "courante")]
    week: String,
    /// Repart de zéro au lieu de compléter les soirs vides.
    #[arg(long)]
    replace: bool,
    /// Affiche sans enregistrer.
    #[arg(long)]
    dry_run: bool,
  },
  /// Serveur MCP (stdio) pour Claude : relaie vers une instance `menus serve`.
  Mcp {
    /// Adresse de l'instance : http://localhost:8080, http://nas:8098…
    #[arg(long, env = "MENUS_URL", default_value = "http://localhost:8080")]
    url: String,
    /// Jeton d'accès de l'instance, si elle en exige un.
    #[arg(long, env = "MENUS_TOKEN", hide_env_values = true)]
    token: Option<String>,
  },
  /// Déclare `menus mcp` dans la configuration de Claude Desktop.
  SetupClaude {
    /// Adresse de l'instance que les outils piloteront.
    #[arg(long, env = "MENUS_URL", default_value = "http://localhost:8080")]
    url: String,
    /// Jeton d'accès de l'instance, si elle en exige un.
    #[arg(long, env = "MENUS_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// Fichier de configuration (défaut : celui de Claude Desktop sur ce système).
    #[arg(long)]
    config: Option<PathBuf>,
  },
}

#[tokio::main]
async fn main() -> Result<()> {
  tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::EnvFilter::try_from_env("MENUS_LOG").unwrap_or_else(|_| "menus=info,tower_http=warn".into()))
    .with_target(false)
    // stdout est réservé au protocole MCP.
    .with_writer(std::io::stderr)
    .init();

  let cli = Cli::parse();
  match cli.command {
    Command::Serve { port, host } => serve(&cli.data, &host, port).await,
    Command::List { week, days } => list_command(&cli.data, &week, days.as_deref()),
    Command::Propose { week, replace, dry_run } => propose_command(&cli.data, &week, replace, dry_run),
    Command::Mcp { url, token } => mcp::run(url, token.map(|t| t.split_whitespace().collect::<String>()).filter(|t| !t.is_empty())).await,
    Command::SetupClaude { url, token, config } => {
      setup::claude_desktop(&url, token.map(|t| t.split_whitespace().collect::<String>()).filter(|t| !t.is_empty()), config)
    }
  }
}

fn list_command(data: &PathBuf, week: &str, days: Option<&str>) -> Result<()> {
  let id = WeekId::parse_loose(week).map_err(anyhow::Error::msg)?;
  let days = model::parse_days(days).map_err(anyhow::Error::msg)?;
  let store = store::Store::open(data)?;
  let settings = store.settings()?;
  let list = shopping::build(&store.week(id)?, &store.recipes()?, &store.catalog()?, &settings, &days);
  if list.total == 0 && list.pantry.is_empty() {
    println!("Rien de prévu pour la semaine {id} ({}).", id.label());
    return Ok(());
  }
  print!("{}", shopping::to_text(&list, &settings));
  for id in &list.missing {
    eprintln!("attention : recette introuvable « {id} »");
  }
  Ok(())
}

fn propose_command(data: &PathBuf, week: &str, replace: bool, dry_run: bool) -> Result<()> {
  let id = WeekId::parse_loose(week).map_err(anyhow::Error::msg)?;
  let store = store::Store::open(data)?;
  let recipes = store.recipes()?;
  let settings = store.settings()?;
  let history = store.history(id)?;
  let planner = planner::Planner {
    recipes: &recipes,
    settings: &settings,
    history: &history,
    month: id.month(),
  };
  let mut current = store.week(id)?;
  let proposed = planner.propose(&current, !replace, &mut rand::rngs::StdRng::from_entropy());

  let name = |rid: &str| recipes.iter().find(|r| r.id == rid).map(|r| r.name.clone()).unwrap_or_else(|| rid.to_string());
  println!("Semaine {id} ({})", id.label());
  for day in model::Day::ALL {
    if let Some(items) = proposed.brunch.get(&day) {
      let names: Vec<String> = items.iter().map(|r| name(r)).collect();
      println!("  {:<9} brunch : {}", day.label(), names.join(" + "));
    }
    let dinner = match proposed.dinner(day) {
      model::Slot::Recipe { recipe } => name(recipe),
      model::Slot::Leftovers => "restes de la veille".into(),
      model::Slot::Free => "libre".into(),
      model::Slot::Empty => "—".into(),
    };
    println!("  {:<9} souper : {dinner}", day.label());
  }
  if dry_run {
    println!("\n--dry-run : rien n'a été écrit.");
    return Ok(());
  }
  current.dinners = proposed.dinners;
  current.brunch = proposed.brunch;
  store.save_week(&mut current)?;
  println!("\nÉcrit : {}", store.week_path(id).display());
  Ok(())
}

async fn serve(data: &PathBuf, host: &str, port: u16) -> Result<()> {
  let store = store::Store::open(data)?;
  let auth_token = std::env::var("MENUS_TOKEN")
    .ok()
    .map(|t| t.split_whitespace().collect::<String>())
    .filter(|t| !t.is_empty());
  let state = app::AppState::new(store, auth_token);

  // Gardée en vie le temps du processus : la surveillance s'arrête avec elle.
  let _watcher = watch::spawn(state.clone())?;

  let router = api::router(state.clone()).merge(web::router()).fallback(web::not_found);

  let address = format!("{host}:{port}");
  let listener = tokio::net::TcpListener::bind(&address).await.with_context(|| format!("écoute sur {address}"))?;

  println!("menus");
  println!(
    "  base      : {}",
    state.store.root().canonicalize().unwrap_or_else(|_| state.store.root().to_path_buf()).display()
  );
  println!(
    "  interface : http://{}",
    if host == "0.0.0.0" { format!("localhost:{port}") } else { address.clone() }
  );
  println!(
    "  accès     : {}",
    if state.auth_token.is_some() {
      "jeton requis (MENUS_TOKEN)"
    } else {
      "ouvert (aucun jeton configuré)"
    }
  );

  axum::serve(listener, router).with_graceful_shutdown(shutdown()).await?;
  Ok(())
}

async fn shutdown() {
  let ctrl_c = async {
    let _ = tokio::signal::ctrl_c().await;
  };
  // Docker arrête un conteneur par SIGTERM.
  #[cfg(unix)]
  let terminate = async {
    if let Ok(mut signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
      signal.recv().await;
    }
  };
  #[cfg(not(unix))]
  let terminate = std::future::pending::<()>();
  tokio::select! {
    _ = ctrl_c => {},
    _ = terminate => {},
  }
  println!("\narrêt.");
}
