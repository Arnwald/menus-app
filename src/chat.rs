//! Le chat de l'app : Claude par l'API Anthropic, avec les mêmes outils que `menus mcp`.
//!
//! Le serveur MCP tourne dans le processus, relié par un tuyau en mémoire, et relaie
//! vers l'API HTTP de l'instance elle-même : ce que Claude change passe par les mêmes
//! validations et apparaît aussitôt sur les téléphones. La clé (`ANTHROPIC_API_KEY`)
//! reste sur le serveur ; le navigateur n'envoie que la conversation.

use crate::api::{ApiError, ApiResult};
use crate::app::AppState;
use crate::mcp;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use rmcp::model::CallToolRequestParams;
use rmcp::ServiceExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const DEFAULT_MODEL: &str = "claude-sonnet-5-5";
/// Allers-retours outil au plus par message : une semaine complète en demande une dizaine.
const MAX_ROUNDS: usize = 16;
/// Messages de la conversation gardés : au-delà, les plus anciens sont oubliés.
const MAX_MESSAGES: usize = 40;

const CHAT_NOTES: &str = "

You are talking with Arnaud or Céline inside the menus app, usually on a phone. Answer in \
French, briefly: a few lines, short lists, no tables and no headings (only **bold** and « - » \
lists are rendered). Don't describe the tools; just use them. Whatever you change shows at once \
in the app's tabs (Semaine, Courses, Recettes), so say what you changed in one line rather than \
repeating the whole plan. Before replacing a week that already has a plan, or deleting \
anything, ask first.";

#[derive(Clone)]
pub struct Chat {
  http: reqwest::Client,
  key: String,
  model: String,
  /// Adresse de l'instance elle-même, pour les outils.
  self_url: String,
  /// API Messages (`MENUS_CLAUDE_URL` pour la remplacer, en test).
  api_url: String,
}

impl Chat {
  /// Le chat n'existe que si une clé est donnée.
  pub fn from_env(port: u16) -> Option<Chat> {
    let key = std::env::var("ANTHROPIC_API_KEY").ok()?.split_whitespace().collect::<String>();
    if key.is_empty() {
      return None;
    }
    let model = std::env::var("MENUS_CLAUDE_MODEL")
      .ok()
      .map(|m| m.trim().to_string())
      .filter(|m| !m.is_empty())
      .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let http = reqwest::Client::builder().timeout(Duration::from_secs(180)).build().ok()?;
    Some(Chat {
      http,
      key,
      model,
      self_url: format!("http://127.0.0.1:{port}"),
      api_url: std::env::var("MENUS_CLAUDE_URL").unwrap_or_else(|_| API_URL.to_string()),
    })
  }

  pub fn model(&self) -> &str {
    &self.model
  }
}

#[derive(Deserialize)]
pub struct ChatRequest {
  /// La conversation au format de l'API : `{role, content}`, content en texte ou en blocs.
  messages: Vec<Value>,
  /// Semaine affichée dans l'app.
  #[serde(default)]
  week: Option<String>,
}

pub async fn info(State(state): State<AppState>) -> Json<Value> {
  Json(match &state.chat {
    Some(chat) => json!({ "enabled": true, "model": chat.model() }),
    None => json!({ "enabled": false }),
  })
}

pub async fn send(State(state): State<AppState>, Json(request): Json<ChatRequest>) -> ApiResult<Json<Value>> {
  let Some(chat) = state.chat.clone() else {
    return Err(ApiError {
      status: StatusCode::SERVICE_UNAVAILABLE,
      message: "le chat n'est pas configuré : donner ANTHROPIC_API_KEY au serveur".into(),
    });
  };
  let mut messages = clean(request.messages)?;

  // Les outils, servis en mémoire par le même code que `menus mcp`.
  let tools = mcp::tools_for(&chat.self_url, state.auth_token.clone())?;
  let (server_io, client_io) = tokio::io::duplex(1 << 16);
  tokio::spawn(async move {
    match tools.serve(server_io).await {
      Ok(service) => {
        let _ = service.waiting().await;
      }
      Err(e) => tracing::warn!("chat : outils indisponibles ({e})"),
    }
  });
  let client = ().serve(client_io).await.map_err(|e| anyhow::anyhow!("outils du chat : {e}"))?;
  let listed = client.list_all_tools().await.map_err(|e| anyhow::anyhow!("outils du chat : {e}"))?;
  let mut tool_specs: Vec<Value> = listed
    .iter()
    .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.input_schema }))
    .collect();
  // Le cache couvre les outils et les consignes : ils ne changent pas d'un message à l'autre.
  if let Some(Value::Object(last)) = tool_specs.last_mut() {
    last.insert("cache_control".into(), json!({ "type": "ephemeral" }));
  }

  let today = chrono::Local::now();
  let context = format!(
    "Today is {} ({}). The app is showing week {}.",
    today.format("%Y-%m-%d"),
    today.format("%A"),
    request.week.as_deref().filter(|w| !w.is_empty()).unwrap_or("courante"),
  );
  let system = json!([
    { "type": "text", "text": format!("{}{}", mcp::INSTRUCTIONS, CHAT_NOTES), "cache_control": { "type": "ephemeral" } },
    { "type": "text", "text": context },
  ]);

  let mut actions: Vec<String> = Vec::new();
  let mut reply = String::new();
  for round in 0..MAX_ROUNDS {
    let body = json!({
      "model": chat.model,
      "max_tokens": 8192,
      "system": system,
      "tools": tool_specs,
      "messages": messages,
    });
    let answer = chat.call(&body).await?;
    let content = answer.get("content").cloned().unwrap_or_else(|| json!([]));
    let stop = answer.get("stop_reason").and_then(Value::as_str).unwrap_or("");
    messages.push(json!({ "role": "assistant", "content": content }));

    let blocks = content.as_array().cloned().unwrap_or_default();
    reply = blocks
      .iter()
      .filter(|b| b["type"] == "text")
      .filter_map(|b| b["text"].as_str())
      .collect::<Vec<_>>()
      .join("\n\n");

    if stop != "tool_use" {
      break;
    }
    if round + 1 == MAX_ROUNDS {
      reply.push_str("\n\n(Je me suis arrêté là : trop d'étapes pour un seul message.)");
      break;
    }

    let mut results = Vec::new();
    for block in blocks.iter().filter(|b| b["type"] == "tool_use") {
      let name = block["name"].as_str().unwrap_or_default().to_string();
      let id = block["id"].clone();
      let mut params = CallToolRequestParams::new(name.clone());
      params.arguments = block["input"].as_object().cloned();
      let (text, is_error) = match client.call_tool(params).await {
        Ok(result) => {
          let value = serde_json::to_value(&result).unwrap_or_default();
          let text = value["content"]
            .as_array()
            .map(|c| c.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();
          (text, value["isError"].as_bool().unwrap_or(false))
        }
        Err(e) => (format!("outil indisponible : {e}"), true),
      };
      tracing::info!("chat : {name}{}", if is_error { " (refusé)" } else { "" });
      if !is_error {
        actions.push(name);
      }
      results.push(json!({ "type": "tool_result", "tool_use_id": id, "content": text, "is_error": is_error }));
    }
    messages.push(json!({ "role": "user", "content": results }));
  }
  let _ = client.cancel().await;

  Ok(Json(json!({ "reply": reply.trim(), "actions": actions })))
}

impl Chat {
  async fn call(&self, body: &Value) -> ApiResult<Value> {
    let response = self
      .http
      .post(&self.api_url)
      .header("x-api-key", &self.key)
      .header("anthropic-version", "2023-06-01")
      .json(body)
      .send()
      .await
      .map_err(|e| bad_gateway(format!("Claude ne répond pas ({e})")))?;
    let status = response.status();
    let value: Value = response.json().await.map_err(|e| bad_gateway(format!("réponse illisible de Claude ({e})")))?;
    if status.is_success() {
      return Ok(value);
    }
    let detail = value["error"]["message"].as_str().unwrap_or("erreur inconnue");
    let message = match status.as_u16() {
      401 | 403 => format!("clé API refusée ({detail})"),
      429 => "trop de demandes à Claude pour l'instant, réessayer dans une minute".into(),
      529 => "Claude est surchargé, réessayer dans un moment".into(),
      _ if detail.contains("credit") => format!("crédits API épuisés ({detail})"),
      _ => format!("Claude a refusé la demande ({status} : {detail})"),
    };
    Err(bad_gateway(message))
  }
}

fn bad_gateway(message: String) -> ApiError {
  tracing::warn!("chat : {message}");
  ApiError {
    status: StatusCode::BAD_GATEWAY,
    message,
  }
}

/// Garde une conversation que l'API accepte : rôles user/assistant qui alternent,
/// le premier et le dernier de l'utilisateur, et pas plus de `MAX_MESSAGES`.
fn clean(messages: Vec<Value>) -> ApiResult<Vec<Value>> {
  let mut kept: Vec<Value> = Vec::new();
  for message in messages {
    let role = message["role"].as_str().unwrap_or_default();
    if role != "user" && role != "assistant" {
      return Err(ApiError::bad_request(format!("rôle inconnu : « {role} »")));
    }
    let content = message["content"].clone();
    let empty = match &content {
      Value::String(s) => s.trim().is_empty(),
      Value::Array(a) => a.is_empty(),
      _ => true,
    };
    if empty {
      continue;
    }
    match kept.last_mut() {
      // Deux messages de suite du même rôle : on les fusionne.
      Some(last) if last["role"] == role => {
        let mut blocks = blocks_of(last["content"].take());
        blocks.extend(blocks_of(content));
        last["content"] = Value::Array(blocks);
      }
      _ => kept.push(json!({ "role": role, "content": content })),
    }
  }
  while kept.len() > MAX_MESSAGES || kept.first().is_some_and(|m| m["role"] != "user") {
    kept.remove(0);
  }
  if kept.last().is_none_or(|m| m["role"] != "user") {
    return Err(ApiError::bad_request("la conversation doit finir par un message de l'utilisateur"));
  }
  Ok(kept)
}

fn blocks_of(content: Value) -> Vec<Value> {
  match content {
    Value::String(text) => vec![json!({ "type": "text", "text": text })],
    Value::Array(blocks) => blocks,
    _ => vec![],
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn conversation_is_cleaned() {
    let messages = vec![
      json!({ "role": "assistant", "content": "Bonjour" }),
      json!({ "role": "user", "content": "Planifie" }),
      json!({ "role": "user", "content": "la semaine" }),
      json!({ "role": "assistant", "content": "" }),
    ];
    // L'assistant de tête et le message vide tombent, les deux messages de suite fusionnent.
    let kept = clean(messages).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0]["role"], "user");
    assert_eq!(kept[0]["content"].as_array().unwrap().len(), 2);
  }

  #[test]
  fn must_end_with_the_user() {
    let messages = vec![json!({ "role": "user", "content": "Salut" }), json!({ "role": "assistant", "content": "Bonjour" })];
    assert!(clean(messages).is_err());
  }

  #[test]
  fn unknown_role_is_refused() {
    assert!(clean(vec![json!({ "role": "system", "content": "x" })]).is_err());
  }

  #[tokio::test]
  async fn tools_are_served_in_memory() {
    let tools = mcp::tools_for("http://127.0.0.1:1", None).unwrap();
    let (a, b) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
      if let Ok(s) = tools.serve(a).await {
        let _ = s.waiting().await;
      }
    });
    let client = ().serve(b).await.unwrap();
    let listed = client.list_all_tools().await.unwrap();
    assert!(listed.iter().any(|t| t.name == "plan_week"));
    // L'instance n'existe pas : l'outil répond par une erreur lisible, pas une panique.
    let result = client.call_tool(CallToolRequestParams::new("get_week")).await.unwrap();
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(value["isError"], true);
  }
}
