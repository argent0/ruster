use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::broadcast;
use serde::{Deserialize, Serialize};
use serde_json::json;
use chrono::Local;
use anyhow::{Result, anyhow};
use crate::config::Config;
use crate::llm::LlmClient;
use crate::skills::{SkillsManager, Skill};

#[derive(Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    pub timestamp: String,
    #[serde(default)]
    pub skills: Vec<String>,
}

pub struct Session {
    pub id: String,
    pub history: Vec<Message>,
    pub active_skills: Vec<String>,
    pub model: String, // provider/model
    pub memory_dir: PathBuf,
    pub history_file: PathBuf,
    pub activity_file: PathBuf,
    pub skills_manager: Arc<RwLock<SkillsManager>>,
    pub llm_client: LlmClient,
    pub config: Arc<RwLock<Config>>,
}

impl Session {
    pub async fn new(
        id: String,
        config: Arc<RwLock<Config>>,
        skills_manager: Arc<RwLock<SkillsManager>>,
        llm_client: LlmClient,
        model_override: Option<String>,
    ) -> Result<Self> {
        tracing::info!(session_id = %id, "Initializing session");
        let base_dir = crate::logging::get_log_dir()?.parent().unwrap().join("sessions").join(&id);
        fs::create_dir_all(&base_dir)?;
        fs::create_dir_all(base_dir.join("memory"))?;

        let history_file = base_dir.join("history.jsonl");
        let activity_file = base_dir.join("activity.log");
        
        let history: Vec<Message> = if history_file.exists() {
            tracing::debug!(session_id = %id, "Loading history from {:?}", history_file);
            let content = fs::read_to_string(&history_file)?;
            content.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        } else {
            tracing::debug!(session_id = %id, "No history file found, starting new history");
            Vec::new()
        };

        let active_skills = {
            let cfg = config.read().await;
            cfg.initial_skills.clone()
        };

        let model = if let Some(m) = model_override {
            tracing::debug!(session_id = %id, model = %m, "Using model override");
            m
        } else {
            let cfg = config.read().await;
            tracing::debug!(session_id = %id, model = %cfg.default_model, "Using default model from config");
            cfg.default_model.clone()
        };

        tracing::info!(session_id = %id, history_len = %history.len(), active_skills_count = %active_skills.len(), "Session initialized");

        Ok(Self {
            id,
            history,
            active_skills,
            model,
            memory_dir: base_dir.join("memory"),
            history_file,
            activity_file,
            skills_manager,
            llm_client,
            config,
        })
    }

    pub fn add_user_message(&mut self, content: String, skills: Vec<String>) -> Result<()> {
        self.log_activity(&format!("User: {}", content))?;
        let msg = Message {
            role: "user".to_string(),
            content,
            timestamp: Local::now().to_rfc3339(),
            skills,
        };
        self.history.push(msg.clone());
        self.append_history(&msg)?;
        Ok(())
    }

    pub async fn prepare_context(&self) -> Result<(Vec<serde_json::Value>, Vec<Skill>, Vec<crate::llm::Tool>)> {
        // Detect skills based on last user message
        let last_msg = self.history.last().ok_or_else(|| anyhow!("No history found"))?;
        
        let (rag_model, message_content, banned_skills) = {
            let cfg = self.config.read().await;
            (cfg.rag_model.clone(), last_msg.content.clone(), cfg.banned_skills.clone())
        };
        
        // 1. Get manually enabled skills
        let mut skills = Vec::new();
        {
            let mgr = self.skills_manager.read().await;
            for name in &self.active_skills {
                 if let Some(skill) = mgr.get_skill(name) {
                     skills.push(skill.clone());
                 }
            }
        }

        // 2. Select dynamic skills (RAG)
        let dynamic_skills = {
            let mut mgr = self.skills_manager.write().await;
            mgr.select_skills(&message_content, &self.llm_client, &rag_model).await?
        };

        for ds in dynamic_skills {
            // Only add if not already in active_skills and NOT banned
            if !self.active_skills.contains(&ds.metadata.name) && !banned_skills.contains(&ds.metadata.name) {
                skills.push(ds);
            }
        }

        let mut tools = Vec::new();
        for skill in &skills {
            tools.extend(skill.metadata.tools.clone());
        }

        // Add built-in pagination tool
        tools.push(crate::llm::Tool {
            name: "paginate_tool_output".to_string(),
            description: "Paginates the output of a previous tool call. Use this to see more lines, a specific range, or search for text in the full output of a tool.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "tool_call_uuid": {
                        "type": "string",
                        "description": "The UUID of the tool call to paginate."
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Starting line number (0-indexed)."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Number of lines to return."
                    },
                    "search": {
                        "type": "string",
                        "description": "Optional search term to filter lines."
                    }
                },
                "required": ["tool_call_uuid"]
            }),
            exec: None, // Built-in
            working_dir: None,
        });

        // Add run_skill_script tool
        tools.push(crate::llm::Tool {
            name: "run_skill_script".to_string(),
            description: "Executes a script from an active skill's 'scripts' directory. Provide the skill name, script name (with extension), and an optional 'args' array.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "The name of the skill containing the script."
                    },
                    "script_name": {
                        "type": "string",
                        "description": "The name of the script file (e.g., 'browser-active.sh')."
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Arguments to pass to the script."
                    }
                },
                "required": ["skill_name", "script_name"]
            }),
            exec: None, // Built-in logic in server.rs
            working_dir: None,
        });

        if !skills.is_empty() {
            let names: Vec<_> = skills.iter().map(|s| &s.metadata.name).collect();
            tracing::info!(session_id = %self.id, "Activating skills: {:?}", names);
        } else {
            tracing::debug!(session_id = %self.id, "No relevant skills found for message");
        }

        let mut messages = Vec::new();
        
        let mut system_prompt = String::from("You are Ruster, a persistent, proactive LLM agent.\n");
        if !skills.is_empty() {
            system_prompt.push_str("\n# Enabled Skills:\n");
            for skill in &skills {
                system_prompt.push_str(&format!("## {}\n{}\n", skill.metadata.name, skill.instructions));
            }
        }

        tracing::debug!("System prompt: '{}'", &system_prompt);
        
        messages.push(json!({"role": "system", "content": system_prompt}));
        
        for msg in &self.history {
            messages.push(json!({"role": msg.role, "content": msg.content}));
        }

        Ok((messages, skills, tools))
    }

    pub fn add_assistant_message(&mut self, content: String, skills: Vec<String>) -> Result<()> {
        let msg = Message {
            role: "assistant".to_string(),
            content: content.clone(),
            timestamp: Local::now().to_rfc3339(),
            skills,
        };
        self.history.push(msg.clone());
        self.append_history(&msg)?;
        self.log_activity(&format!("Assistant: {}", content))?;
        Ok(())
    }

    pub fn add_skill(&mut self, name: String) -> Result<()> {
        if !self.active_skills.contains(&name) {
            self.active_skills.push(name);
        }
        Ok(())
    }

    pub fn remove_skill(&mut self, name: &str) -> Result<()> {
        self.active_skills.retain(|s| s != name);
        // Also remove from history
        for msg in &mut self.history {
            msg.skills.retain(|s| s != name);
        }
        self.rewrite_history()?;
        Ok(())
    }

    fn rewrite_history(&self) -> Result<()> {
        let mut file = fs::File::create(&self.history_file)?;
        for msg in &self.history {
            let line = serde_json::to_string(msg)?;
            writeln!(file, "{}", line)?;
        }
        Ok(())
    }

    fn append_history(&self, msg: &Message) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.history_file)?;
        let line = serde_json::to_string(msg)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    fn log_activity(&self, text: &str) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.activity_file)?;
        writeln!(file, "[{}] {}", Local::now(), text)?;
        Ok(())
    }
}

pub struct SessionManager {
    // Stores id -> Arc<RwLock<Session>>
    // Since we need to modify the map (add/remove), RwLock<HashMap>
    pub sessions: RwLock<HashMap<String, Arc<RwLock<Session>>>>,
    pub config: Arc<RwLock<Config>>,
    pub skills_manager: Arc<RwLock<SkillsManager>>,
    pub llm_client: LlmClient,
    pub event_sender: broadcast::Sender<serde_json::Value>,
}

impl SessionManager {
    pub fn new(config: Arc<RwLock<Config>>, skills_manager: Arc<RwLock<SkillsManager>>, llm_client: LlmClient) -> Self {
        let (tx, _) = broadcast::channel(100);
        Self {
            sessions: RwLock::new(HashMap::new()),
            config,
            skills_manager,
            llm_client,
            event_sender: tx,
        }
    }

    pub async fn get_session(&self, id: &str) -> Result<Arc<RwLock<Session>>> {
        {
            let map = self.sessions.read().await;
            if let Some(session) = map.get(id) {
                return Ok(session.clone());
            }
        }
        
        let session = Session::new(
            id.to_string(),
            self.config.clone(),
            self.skills_manager.clone(),
            self.llm_client.clone(),
            None,
        ).await?;
        
        let session_arc = Arc::new(RwLock::new(session));
        
        let mut map = self.sessions.write().await;
        map.insert(id.to_string(), session_arc.clone());
        
        Ok(session_arc)
    }

    pub async fn list_sessions(&self) -> Result<Vec<String>> {
        // Only return currently loaded or find on disk?
        // Let's find on disk + loaded.
        let mut ids = Vec::new();
        
        // From disk
        let log_dir = crate::logging::get_log_dir()?;
        let sessions_dir = log_dir.parent().unwrap().join("sessions");
        if sessions_dir.exists() {
             for entry in fs::read_dir(sessions_dir)? {
                 let entry = entry?;
                 if entry.file_type()?.is_dir() {
                     ids.push(entry.file_name().to_string_lossy().to_string());
                 }
             }
        }
        
        // From memory (might be new but not saved yet?)
        let map = self.sessions.read().await;
        for key in map.keys() {
            if !ids.contains(key) {
                ids.push(key.clone());
            }
        }
        
        Ok(ids)
    }

    pub async fn delete_session(&self, id: &str) -> Result<()> {
        {
            let mut map = self.sessions.write().await;
            map.remove(id);
        }
        
        let log_dir = crate::logging::get_log_dir()?;
        let session_dir = log_dir.parent().unwrap().join("sessions").join(id);
        if session_dir.exists() {
            fs::remove_dir_all(session_dir)?;
        }
        Ok(())
    }
}
