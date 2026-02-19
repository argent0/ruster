use tokio::net::{UnixListener, UnixStream};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use std::sync::Arc;
use serde_json::{json, Value, Map};
use serde::Deserialize;
use futures_util::StreamExt;
use crate::session::SessionManager;
use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum CommandRequest {
    Dsl {
        command: String,
        arguments: Value,
    },
    Legacy {
        action: String,
        #[serde(flatten)]
        args: Map<String, Value>,
    },
}

pub async fn start_server(socket_path: &str, session_manager: Arc<SessionManager>) -> Result<()> {
    if Path::new(socket_path).exists() {
        fs::remove_file(socket_path)?;
    }

    let listener = UnixListener::bind(socket_path)?;
    // Set permissions to 0666
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(socket_path, fs::Permissions::from_mode(0o666))?;

    tracing::info!("Listening on {}", socket_path);

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let sm = session_manager.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, sm).await {
                        tracing::error!("Connection error: {}", e);
                    }
                });
            }
            Err(e) => {
                tracing::error!("Accept error: {}", e);
            }
        }
    }
}

async fn handle_connection(stream: UnixStream, session_manager: Arc<SessionManager>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    
    let (tx, mut rx) = mpsc::channel::<Value>(100);
    
    // Subscribe to broadcast events
    let mut broadcast_rx = session_manager.event_sender.subscribe();

    // Spawn writer task
    tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Some(val) => {
                            let s = val.to_string();
                            if let Err(_) = writer.write_all(s.as_bytes()).await { break; }
                            if let Err(_) = writer.write_all(b"
").await { break; }
                            if let Err(_) = writer.flush().await { break; }
                        }
                        None => break, // Channel closed
                    }
                }
                res = broadcast_rx.recv() => {
                    match res {
                        Ok(val) => {
                            let s = val.to_string();
                            if let Err(_) = writer.write_all(s.as_bytes()).await { break; }
                            if let Err(_) = writer.write_all(b"
").await { break; }
                            if let Err(_) = writer.flush().await { break; }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    });

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() { continue; }
        
        let req: CommandRequest = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(json!({"error": format!("Invalid JSON or Command format: {}", e)})).await;
                continue;
            }
        };

        let sm = session_manager.clone();
        let tx_clone = tx.clone();
        
        // Handle command
        tokio::spawn(async move {
             if let Err(e) = process_command(req, sm, tx_clone).await {
                 tracing::error!("Command processing error: {}", e);
             }
        });
    }

    Ok(())
}

async fn process_command(req: CommandRequest, sm: Arc<SessionManager>, tx: mpsc::Sender<Value>) -> Result<()> {
    match req {
        CommandRequest::Dsl { command, arguments } => {
            match command.as_str() {
                "session" => {
                    let action = arguments["action"].as_str().ok_or_else(|| anyhow!("Missing action in session arguments"))?;
                    handle_session_action(action, arguments.clone(), sm, tx).await
                },
                "config" => {
                    let action = arguments["action"].as_str().ok_or_else(|| anyhow!("Missing action in config arguments"))?;
                    handle_config_action(action, arguments.clone(), sm, tx).await
                },
                "skill" => {
                    let action = arguments["action"].as_str().ok_or_else(|| anyhow!("Missing action in skill arguments"))?;
                    handle_skill_action(action, arguments.clone(), sm, tx).await
                },
                _ => {
                    tx.send(json!({"error": format!("Unknown command: {}", command)})).await.map_err(|_| anyhow!("Send failed"))?;
                    Ok(())
                }
            }
        },
        CommandRequest::Legacy { action, args } => {
            // Convert Map<String, Value> to Value (Object)
            let arguments = Value::Object(args);
            if action.starts_with("skill_") {
                let stripped = action.strip_prefix("skill_").unwrap();
                handle_skill_action(stripped, arguments, sm, tx).await
            } else {
                handle_session_action(&action, arguments, sm, tx).await
            }
        }
    }
}

async fn handle_skill_action(action: &str, args: Value, sm: Arc<SessionManager>, tx: mpsc::Sender<Value>) -> Result<()> {
    let session_id = args["session_id"].as_str().ok_or_else(|| anyhow!("Missing session_id"))?;
    let session_arc = sm.get_session(session_id).await?;

    match action {
        "add" => {
            let skill_name = args["skill"].as_str().ok_or_else(|| anyhow!("Missing skill name"))?;
            let mut session = session_arc.write().await;
            session.add_skill(skill_name.to_string())?;
            tx.send(json!({
                "event": "skill_added",
                "session_id": session_id,
                "skill": skill_name
            })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        "remove" => {
            let skill_name = args["skill"].as_str().ok_or_else(|| anyhow!("Missing skill name"))?;
            let mut session = session_arc.write().await;
            session.remove_skill(skill_name)?;
            tx.send(json!({
                "event": "skill_removed",
                "session_id": session_id,
                "skill": skill_name
            })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        "list" => {
             // List skills currently in session
             let session = session_arc.read().await;
             tx.send(json!({
                 "event": "skill_list",
                 "session_id": session_id,
                 "active_skills": session.active_skills
             })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        "search" => {
            let query = args["query"].as_str().ok_or_else(|| anyhow!("Missing query"))?;
            let mut mgr = sm.skills_manager.write().await;
            let results = mgr.search_skills(query, &sm.llm_client, &sm.config.read().await.rag_model).await?;
            let metadata: Vec<_> = results.iter().map(|s| &s.metadata).collect();
            tx.send(json!({
                "event": "skill_search_results",
                "session_id": session_id,
                "results": metadata
            })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        "ban" => {
            let skill_name = args["skill"].as_str().ok_or_else(|| anyhow!("Missing skill name"))?;
            {
                let mut config = sm.config.write().await;
                if !config.banned_skills.contains(&skill_name.to_string()) {
                    config.banned_skills.push(skill_name.to_string());
                    config.save()?;
                }
            }
            tx.send(json!({
                "event": "skill_banned",
                "session_id": session_id,
                "skill": skill_name
            })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        "unban" => {
            let skill_name = args["skill"].as_str().ok_or_else(|| anyhow!("Missing skill name"))?;
            {
                let mut config = sm.config.write().await;
                config.banned_skills.retain(|s| s != skill_name);
                config.save()?;
            }
            tx.send(json!({
                "event": "skill_unbanned",
                "session_id": session_id,
                "skill": skill_name
            })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        _ => {
            tx.send(json!({"error": format!("Unknown skill action: {}", action)})).await.map_err(|_| anyhow!("Send failed"))?;
        }
    }
    Ok(())
}

async fn handle_config_action(action: &str, args: Value, sm: Arc<SessionManager>, tx: mpsc::Sender<Value>) -> Result<()> {
    match action {
        "set" => {
            let key = args["key"].as_str().ok_or_else(|| anyhow!("Missing key"))?;
            let val = args["value"].clone();
            if val.is_null() { return Err(anyhow!("Missing value")); }
            
            {
                let mut config = sm.config.write().await;
                config.set_value(key, val.clone())?;
            }
            
            tx.send(json!({
                "event": "config_updated",
                "key": key,
                "value": val
            })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        "get" => {
            let key = args["key"].as_str().ok_or_else(|| anyhow!("Missing key"))?;
            let val = {
                let config = sm.config.read().await;
                config.get_value(key)?
            };
            tx.send(json!({
                "event": "config_value",
                "key": key,
                "value": val
            })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        "list" => {
            let keys = crate::config::Config::get_keys();
            let mut values = Map::new();
            {
                let config = sm.config.read().await;
                for key in &keys {
                    if let Ok(v) = config.get_value(key) {
                        values.insert(key.clone(), v);
                    }
                }
            }
            tx.send(json!({
                "event": "config_list",
                "options": values
            })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        _ => {
             tx.send(json!({"error": format!("Unknown config action: {}", action)})).await.map_err(|_| anyhow!("Send failed"))?;
        }
    }
    Ok(())
}

async fn handle_session_action(action: &str, req: Value, sm: Arc<SessionManager>, tx: mpsc::Sender<Value>) -> Result<()> {
    match action {
        "create" => {
            let session_id = req["session_id"].as_str().ok_or_else(|| anyhow!("Missing session_id"))?;
            let model = req["model"].as_str(); // Optional override
            
            // Check if exists
            {
                // We just call get_session which creates it.
                // But we might want to fail if it exists? Spec doesn't say.
                // Assuming idempotent create or switch.
                // "Create" usually implies making new.
            }
            // Just ensure it's loaded/created.
            let session_arc = sm.get_session(session_id).await?;
            // If model provided, update it?
            if let Some(m) = model {
                let mut session = session_arc.write().await;
                session.model = m.to_string();
            }
            
            let final_model = {
                let session = session_arc.read().await;
                session.model.clone()
            };
            
            tx.send(json!({
                "event": "created",
                "session_id": session_id,
                "model": final_model
            })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        "send" => {
            let session_id = req["session_id"].as_str().ok_or_else(|| anyhow!("Missing session_id"))?;
            let message = req["message"].as_str().ok_or_else(|| anyhow!("Missing message"))?;
            
            let session_arc = sm.get_session(session_id).await?;
            
            // 1. Add user message
            {
                let mut session = session_arc.write().await;
                // Get currently active skills to tag message
                let current_skills = session.active_skills.clone();
                session.add_user_message(message.to_string(), current_skills)?;
            }
            
            // 2. Prepare context (detect skills)
            let (context, skills) = {
                let session = session_arc.read().await;
                session.prepare_context().await?
            };
            
            if !skills.is_empty() {
                let names: Vec<_> = skills.iter().map(|s| &s.metadata.name).collect();
                tracing::info!(session_id = %session_id, skills = ?names, "LLM starting generation with skills enabled.");
            } else {
                tracing::info!(session_id = %session_id, "LLM starting generation (no skills).");
            }
            
            // Notify detected skills? Spec: {"event":"skill_used",...}
            // "If relevant, execute skill(s), inject results into context".
            // "Server -> Client events ... skill_used".
            // Since we just inject instructions, maybe we emit "skill_used" here.
            for skill in &skills {
                tx.send(json!({
                    "event": "skill_used",
                    "session_id": session_id,
                    "skill": skill.metadata.name,
                    "result": "Skill instructions injected." 
                })).await.map_err(|_| anyhow!("Send failed"))?;
            }

            // 3. Call LLM stream
            let model_str = {
                let session = session_arc.read().await;
                session.model.clone()
            };
            
            // Note: process_command is async, holding session_arc might block others if we held lock.
            // But we dropped locks.
            
            let mut stream = sm.llm_client.chat_stream(&model_str, context, None).await?;
            
            let mut full_response = String::new();
            
            tx.send(json!({
                "event": "response",
                "session_id": session_id,
                "delta": "Thinking...",
                "done": false
            })).await.map_err(|_| anyhow!("Send failed"))?;
            
            while let Some(chunk_res) = stream.next().await {
                match chunk_res {
                    Ok(chunk) => {
                        full_response.push_str(&chunk);
                        tx.send(json!({
                            "event": "response",
                            "session_id": session_id,
                            "delta": chunk,
                            "done": false
                        })).await.map_err(|_| anyhow!("Send failed"))?;
                    },
                    Err(e) => {
                        tracing::error!(session_id = %session_id, error = %e, "LLM Stream Error occurred.");
                        tx.send(json!({
                            "error": format!("LLM Stream Error: {}", e),
                            "session_id": session_id
                        })).await.map_err(|_| anyhow!("Send failed"))?;
                        break;
                    }
                }
            }
            
            tx.send(json!({
                "event": "response",
                "session_id": session_id,
                "delta": "", // Final delta empty? Or "Final answer."? Spec says "delta":"Final answer.","done":true
                // But we streamed the answer.
                // Usually "done":true comes with empty delta or just marks end.
                // I'll send empty delta with done=true.
                "done": true
            })).await.map_err(|_| anyhow!("Send failed"))?;
            
            // 4. Add assistant message
            {
                let mut session = session_arc.write().await;
                // Current active skills might have changed? No, not during generation.
                let current_skills = session.active_skills.clone();
                session.add_assistant_message(full_response, current_skills)?;
            }
        },
        "list" => {
            let sessions = sm.list_sessions().await?;
            tx.send(json!({
                "event": "list",
                "sessions": sessions
            })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        "delete" => {
            let session_id = req["session_id"].as_str().ok_or_else(|| anyhow!("Missing session_id"))?;
            sm.delete_session(session_id).await?;
            tx.send(json!({
                "event": "deleted",
                "session_id": session_id
            })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        "history" => {
            let session_id = req["session_id"].as_str().ok_or_else(|| anyhow!("Missing session_id"))?;
            let limit = req["limit"].as_u64().unwrap_or(20) as usize;
            let offset = req["offset"].as_u64().unwrap_or(0) as usize;
            
            let session_arc = sm.get_session(session_id).await?;
            let session = session_arc.read().await;
            
            let total = session.history.len();
            let start = offset.min(total);
            let end = (offset + limit).min(total);
            let slice = &session.history[start..end];
            
            tx.send(json!({
                "event": "history",
                "session_id": session_id,
                "history": slice,
                "total": total,
                "offset": offset,
                "limit": limit
            })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        _ => {
            tx.send(json!({"error": format!("Unknown action: {}", action)})).await.map_err(|_| anyhow!("Send failed"))?;
        }
    }
    
    Ok(())
}
