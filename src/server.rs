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
use uuid::Uuid;
use std::process::Stdio;
use tokio::process::Command;
use std::io::Write as _;

#[derive(Deserialize, Debug)]
struct CommandRequest {
    command: String,
    arguments: Value,
}

async fn execute_tool(
    call: crate::llm::ToolCall,
    tools: &[crate::llm::Tool],
    skills: &[crate::skills::Skill],
    config: &crate::config::Config,
    user_msg: &str,
    assistant_resp: &str,
) -> Result<(String, String)> {
    let tool_uuid = Uuid::new_v4().to_string();
    let expanded_tool_run_dir = crate::config::expand_path(&config.tool_run_dir);
    let tool_run_dir = expanded_tool_run_dir.join("tools").join(&tool_uuid);
    fs::create_dir_all(&tool_run_dir)?;

    let call_log_path = tool_run_dir.join("call");
    let mut call_log = fs::File::create(call_log_path)?;
    writeln!(call_log, "Timestamp: {}", chrono::Local::now())?;
    writeln!(call_log, "Tool Call UUID: {}", tool_uuid)?;
    writeln!(call_log, "Tool Name: {}", call.name)?;
    writeln!(call_log, "Arguments: {}", call.arguments)?;
    writeln!(call_log, "User Message: {}", user_msg)?;
    writeln!(call_log, "Assistant Response: {}", assistant_resp)?;

    let stdout_all;
    let mut stderr_all = String::new();

    if call.name == "paginate_tool_output" {
        let args: Value = serde_json::from_str(&call.arguments)?;
        let target_uuid = args["tool_call_uuid"].as_str().ok_or_else(|| anyhow!("Missing tool_call_uuid"))?;
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(config.tool_output_lines as u64) as usize;
        let search = args["search"].as_str();

        let target_stdout_path = expanded_tool_run_dir.join("tools").join(target_uuid).join("stdout");
        if !target_stdout_path.exists() {
            return Ok((tool_uuid, format!("Error: Tool run {} not found.", target_uuid)));
        }

        let content = fs::read_to_string(target_stdout_path)?;
        let mut lines: Vec<_> = content.lines().collect();
        
        if let Some(term) = search {
            lines.retain(|l| l.contains(term));
        }

        let total = lines.len();
        let start = offset.min(total);
        let end = (offset + limit).min(total);
        let sliced = &lines[start..end];

        let mut res = sliced.join("\n");
        if end < total {
            res.push_str(&format!("\n\n(Showing lines {}-{} of {}. Use paginate_tool_output for more.)", start, end, total));
        }
        stdout_all = res;
    } else if call.name == "run_skill_script" {
        let args: Value = serde_json::from_str(&call.arguments)?;
        let skill_name = args["skill_name"].as_str().ok_or_else(|| anyhow!("Missing skill_name"))?;
        let script_name = args["script_name"].as_str().ok_or_else(|| anyhow!("Missing script_name"))?;
        
        if let Some(skill) = skills.iter().find(|s| s.metadata.name == skill_name) {
            let script_path = skill.path.join("scripts").join(script_name);
            if !script_path.exists() {
                return Ok((tool_uuid, format!("Error: Script '{}' not found in skill '{}'.", script_name, skill_name)));
            }

            let mut cmd = Command::new("bash");
            cmd.arg("-c");
            
            let mut full_cmd = format!("./scripts/{}", script_name);
            if let Some(args_arr) = args["args"].as_array() {
                for arg in args_arr {
                    if let Some(s) = arg.as_str() {
                        full_cmd.push(' ');
                        full_cmd.push_str(&format!("'{}'", s.replace("'", "'\\''")));
                    }
                }
            }
            cmd.arg(full_cmd);
            cmd.current_dir(&skill.path);

            let child = cmd
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            let output = child.wait_with_output().await?;
            stdout_all = String::from_utf8_lossy(&output.stdout).to_string();
            stderr_all = String::from_utf8_lossy(&output.stderr).to_string();
        } else {
            return Ok((tool_uuid, format!("Error: Skill '{}' not found or not active.", skill_name)));
        }
    } else if let Some(tool_def) = tools.iter().find(|t| t.name == call.name) {
        if let Some(exec_cmd) = &tool_def.exec {
            let mut cmd = Command::new("bash");
            cmd.arg("-c");
            
            let mut full_cmd = exec_cmd.clone();
            let args_json: Value = serde_json::from_str(&call.arguments).unwrap_or(json!({}));
            if let Some(args_arr) = args_json["args"].as_array() {
                for arg in args_arr {
                    if let Some(s) = arg.as_str() {
                        full_cmd.push(' ');
                        // Basic shell escaping: wrap in single quotes and escape any single quotes within the string
                        full_cmd.push_str(&format!("'{}'", s.replace("'", "'\\''")));
                    }
                }
            }
            cmd.arg(full_cmd);
            
            if let Some(cwd) = &tool_def.working_dir {
                cmd.current_dir(cwd);
            }

            let child = cmd
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            let output = child.wait_with_output().await?;
            stdout_all = String::from_utf8_lossy(&output.stdout).to_string();
            stderr_all = String::from_utf8_lossy(&output.stderr).to_string();
        } else {
            stdout_all = format!("Error: Tool {} has no execution logic defined.", call.name);
        }
    } else {
        stdout_all = format!("Error: Tool {} not found.", call.name);
    }

    fs::write(tool_run_dir.join("stdout"), &stdout_all)?;
    fs::write(tool_run_dir.join("stderr"), &stderr_all)?;

    let mut result_summary = stdout_all.lines().take(config.tool_output_lines).collect::<Vec<_>>().join("\n");
    if stdout_all.lines().count() > config.tool_output_lines {
        result_summary.push_str(&format!("\n\n(Output truncated. Full output saved in tool run {}. Use paginate_tool_output to see more.)", tool_uuid));
    }

    if !stderr_all.is_empty() {
        result_summary.push_str("\n\nStderr:\n");
        result_summary.push_str(&stderr_all.lines().take(5).collect::<Vec<_>>().join("\n"));
    }

    Ok((tool_uuid, result_summary))
}

pub async fn start_server(socket_path: &str, session_manager: Arc<SessionManager>) -> Result<()> {
    if Path::new(socket_path).exists() {
        if let Err(e) = fs::remove_file(socket_path) {
            return Err(anyhow!(
                "Failed to remove existing socket at {}: {}. \
                 If it belongs to another user (e.g., root), try removing it with sudo.",
                socket_path, e
            ));
        }
    }

    let listener = UnixListener::bind(socket_path).map_err(|e| {
        anyhow!(
            "Failed to bind to socket {}: {}. \
             Ensure you have write permissions to the directory.",
            socket_path, e
        )
    })?;

    // Set permissions to 0666 so other users can connect (if needed)
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = fs::set_permissions(socket_path, fs::Permissions::from_mode(0o666)) {
        tracing::warn!("Could not set permissions on socket {}: {}. This might prevent other users from connecting.", socket_path, e);
    }

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
    let peer_addr = stream.peer_addr().ok();
    tracing::info!(peer_addr = ?peer_addr, "New connection established");
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
            Ok(v) => {
                tracing::debug!(peer_addr = ?peer_addr, "Received command");
                v
            },
            Err(e) => {
                tracing::warn!(peer_addr = ?peer_addr, error = %e, line = %line, "Received invalid JSON");
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

    tracing::info!(peer_addr = ?peer_addr, "Connection closed");
    Ok(())
}

async fn process_command(req: CommandRequest, sm: Arc<SessionManager>, tx: mpsc::Sender<Value>) -> Result<()> {
    let command = req.command;
    let arguments = req.arguments;
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
}

async fn handle_skill_action(action: &str, args: Value, sm: Arc<SessionManager>, tx: mpsc::Sender<Value>) -> Result<()> {
    let session_id = args["session_id"].as_str().ok_or_else(|| anyhow!("Missing session_id"))?;
    tracing::info!(session_id = %session_id, action = %action, "Processing skill action");
    let session_arc = sm.get_session(session_id).await?;

    match action {
        "add" => {
            let skill_name = args["skill"].as_str().ok_or_else(|| anyhow!("Missing skill name"))?;
            tracing::info!(session_id = %session_id, skill = %skill_name, "Adding skill to session");
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
            tracing::info!(session_id = %session_id, skill = %skill_name, "Removing skill from session");
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
             tracing::debug!(session_id = %session_id, "Listing session skills");
             let session = session_arc.read().await;
             tx.send(json!({
                 "event": "skill_list",
                 "session_id": session_id,
                 "active_skills": session.active_skills
             })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        "search" => {
            let query = args["query"].as_str().ok_or_else(|| anyhow!("Missing query"))?;
            tracing::info!(session_id = %session_id, query = %query, "Searching for skills");
            let mut mgr = sm.skills_manager.write().await;
            let (rag_model, top_n, threshold) = {
                let cfg = sm.config.read().await;
                (cfg.rag_model.clone(), cfg.rag_top_n, cfg.rag_threshold)
            };
            let results = mgr.search_skills(query, &sm.llm_client, &rag_model, top_n, threshold).await?;
            let metadata: Vec<_> = results.iter().map(|s| &s.metadata).collect();
            tx.send(json!({
                "event": "skill_search_results",
                "session_id": session_id,
                "results": metadata
            })).await.map_err(|_| anyhow!("Send failed"))?;
        },
        "ban" => {
            let skill_name = args["skill"].as_str().ok_or_else(|| anyhow!("Missing skill name"))?;
            tracing::info!(skill = %skill_name, "Banning skill globally");
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
            tracing::info!(skill = %skill_name, "Unbanning skill globally");
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
    tracing::info!(action = %action, "Processing session action");
    match action {
        "create" => {
            let session_id = req["session_id"].as_str().ok_or_else(|| anyhow!("Missing session_id"))?;
            tracing::info!(session_id = %session_id, "Creating/loading session");
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
            
            tracing::info!(session_id = %session_id, "Handling message send");
            let session_arc = sm.get_session(session_id).await?;
            
            // 1. Add user message
            {
                let mut session = session_arc.write().await;
                // Get currently active skills to tag message
                let current_skills = session.active_skills.clone();
                session.add_user_message(message.to_string(), current_skills).await?;
            }
            
            // 2. Prepare context (detect skills)
            let (context, skills, tools) = {
                let session = session_arc.read().await;
                session.prepare_context().await?
            };
            
            if !skills.is_empty() {
                let names: Vec<_> = skills.iter().map(|s| &s.metadata.name).collect();
                tracing::info!(session_id = %session_id, skills = ?names, "LLM starting generation with skills enabled.");
            } else {
                tracing::info!(session_id = %session_id, "LLM starting generation (no skills).");
            }
            
            for skill in &skills {
                tracing::debug!(session_id = %session_id, skill = %skill.metadata.name, "Skill instructions injected into context");
                tx.send(json!({
                    "event": "skill_used",
                    "session_id": session_id,
                    "skill": skill.metadata.name,
                    "result": "Skill instructions injected." 
                })).await.map_err(|_| anyhow!("Send failed"))?;
            }

            // 3. Call LLM stream (with tool loop)
            let model_str = {
                let session = session_arc.read().await;
                session.model.clone()
            };
            
            let mut context = context;
            let mut full_response = String::new();
            let mut iteration = 0;
            let max_iterations = 10;

            loop {
                iteration += 1;
                if iteration > max_iterations {
                    tracing::warn!(session_id = %session_id, "Max tool iterations reached");
                    break;
                }

                tracing::info!(session_id = %session_id, iteration = %iteration, model = %model_str, "Starting LLM stream");
                
                let mut stream = sm.llm_client.chat_stream(&model_str, context.clone(), if tools.is_empty() { None } else { Some(tools.clone()) }, None).await?;
                
                let mut current_text = String::new();
                let mut tool_calls_this_turn = Vec::<crate::llm::ToolCall>::new();

                tx.send(json!({
                    "event": "response",
                    "session_id": session_id,
                    "delta": if iteration == 1 { "Thinking..." } else { "Refining..." },
                    "done": false
                })).await.map_err(|_| anyhow!("Send failed"))?;
                
                while let Some(chunk_res) = stream.next().await {
                    match chunk_res {
                        Ok(response) => {
                            match response {
                                crate::llm::LlmResponse::Text(chunk) => {
                                    current_text.push_str(&chunk);
                                    tx.send(json!({
                                        "event": "response",
                                        "session_id": session_id,
                                        "delta": chunk,
                                        "done": false
                                    })).await.map_err(|_| anyhow!("Send failed"))?;
                                },
                                crate::llm::LlmResponse::ToolCall(mut call) => {
                                    // Ensure unique ID for tool call if provider doesn't give one
                                    if call.id == "ollama" || call.id == "gemini" || call.id.is_empty() {
                                        call.id = format!("call_{}", Uuid::new_v4().to_string()[..8].to_string());
                                    }
                                    tool_calls_this_turn.push(call);
                                }
                            }
                        },
                        Err(e) => {
                            tracing::error!(session_id = %session_id, error = %e, "LLM Stream Error occurred.");
                            tx.send(json!({
                                "error": format!("LLM Stream Error: {}", e),
                                "session_id": session_id
                            })).await.map_err(|_| anyhow!("Send failed"))?;
                            return Err(e);
                        }
                    }
                }

                if !current_text.is_empty() {
                    if !full_response.is_empty() {
                        full_response.push_str("\n");
                    }
                    full_response.push_str(&current_text);
                }

                if tool_calls_this_turn.is_empty() {
                    break;
                }

                // Add assistant's tool calls to context
                let mut assistant_msg = json!({
                    "role": "assistant",
                    "content": if current_text.is_empty() { Value::Null } else { json!(current_text) }
                });
                
                let provider = model_str.split('/').next().unwrap_or("");
                let tool_calls_json: Vec<_> = tool_calls_this_turn.iter().map(|tc| {
                    if provider == "ollama" || provider == "gemini" {
                        // Ollama and Gemini expect arguments as a JSON object.
                        let args_value: Value = serde_json::from_str(&tc.arguments).unwrap_or(json!(tc.arguments));
                        json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": args_value
                            }
                        })
                    } else {
                        // OpenAI/xAI expects arguments as a JSON string, not an object.
                        json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": tc.arguments
                            }
                        })
                    }
                }).collect();
                assistant_msg["tool_calls"] = json!(tool_calls_json);
                context.push(assistant_msg);

                // Execute tools and add results to context
                for call in tool_calls_this_turn {
                    tracing::info!(session_id = %session_id, tool = %call.name, "LLM requested tool call");
                    
                    let config = sm.config.read().await.clone();
                    let (tool_uuid, result) = match execute_tool(call.clone(), &tools, &skills, &config, message, &full_response).await {
                        Ok(res) => res,
                        Err(e) => (Uuid::new_v4().to_string(), format!("Error executing tool: {}", e)),
                    };

                    tx.send(json!({
                        "event": "tool_call",
                        "session_id": session_id,
                        "tool": call.name,
                        "arguments": call.arguments,
                        "call_id": tool_uuid,
                        "result_preview": result
                    })).await.map_err(|_| anyhow!("Send failed"))?;
                    
                    full_response.push_str(&format!("\n[Tool Call: {} (ID: {}) resulted in:\n{}]", call.name, tool_uuid, result));

                    context.push(json!({
                        "role": "tool",
                        "tool_call_id": call.id,
                        "name": call.name,
                        "content": result
                    }));
                }
            }
            
            tracing::info!(session_id = %session_id, response_len = %full_response.len(), "LLM stream completed");
            
            tx.send(json!({
                "event": "response",
                "session_id": session_id,
                "delta": "", 
                "done": true
            })).await.map_err(|_| anyhow!("Send failed"))?;
            
            // 4. Add assistant message
            {
                let mut session = session_arc.write().await;
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
