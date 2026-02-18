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
}

pub struct Session {
    pub id: String,
    pub history: Vec<Message>,
    pub model: String, // provider/model
    pub memory_dir: PathBuf,
    pub history_file: PathBuf,
    pub activity_file: PathBuf,
    pub skills_manager: Arc<RwLock<SkillsManager>>,
    pub llm_client: LlmClient,
    pub config: Config,
}

impl Session {
    pub async fn new(
        id: String,
        config: Config,
        skills_manager: Arc<RwLock<SkillsManager>>,
        llm_client: LlmClient,
        model_override: Option<String>,
    ) -> Result<Self> {
        let base_dir = crate::logging::get_log_dir()?.parent().unwrap().join("sessions").join(&id);
        fs::create_dir_all(&base_dir)?;
        fs::create_dir_all(base_dir.join("memory"))?;

        let history_file = base_dir.join("history.jsonl");
        let activity_file = base_dir.join("activity.log");
        
        let history = if history_file.exists() {
            let content = fs::read_to_string(&history_file)?;
            content.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        } else {
            Vec::new()
        };

        let model = model_override.unwrap_or_else(|| config.default_model.clone());

        Ok(Self {
            id,
            history,
            model,
            memory_dir: base_dir.join("memory"),
            history_file,
            activity_file,
            skills_manager,
            llm_client,
            config,
        })
    }

    pub fn add_user_message(&mut self, content: String) -> Result<()> {
        self.log_activity(&format!("User: {}", content))?;
        let msg = Message {
            role: "user".to_string(),
            content,
            timestamp: Local::now().to_rfc3339(),
        };
        self.history.push(msg.clone());
        self.append_history(&msg)?;
        Ok(())
    }

    pub async fn prepare_context(&self) -> Result<(Vec<serde_json::Value>, Vec<Skill>)> {
        // Detect skills based on last user message
        let last_msg = self.history.last().ok_or_else(|| anyhow!("No history found"))?;
        
        let skills = {
            let mgr = self.skills_manager.read().await;
            mgr.select_skills(&last_msg.content, &self.llm_client).await?
        };

        let mut messages = Vec::new();
        
        let mut system_prompt = String::from("You are Ruster, a persistent, proactive LLM agent.\n");
        if !skills.is_empty() {
            system_prompt.push_str("\n# Enabled Skills:\n");
            for skill in &skills {
                system_prompt.push_str(&format!("## {}\n{}\n", skill.metadata.name, skill.instructions));
            }
        }
        
        messages.push(json!({"role": "system", "content": system_prompt}));
        
        for msg in &self.history {
            messages.push(json!({"role": msg.role, "content": msg.content}));
        }

        Ok((messages, skills))
    }

    pub fn add_assistant_message(&mut self, content: String) -> Result<()> {
        let msg = Message {
            role: "assistant".to_string(),
            content: content.clone(),
            timestamp: Local::now().to_rfc3339(),
        };
        self.history.push(msg.clone());
        self.append_history(&msg)?;
        self.log_activity(&format!("Assistant: {}", content))?;
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
    pub config: Config,
    pub skills_manager: Arc<RwLock<SkillsManager>>,
    pub llm_client: LlmClient,
    pub event_sender: broadcast::Sender<serde_json::Value>,
}

impl SessionManager {
    pub fn new(config: Config, skills_manager: Arc<RwLock<SkillsManager>>, llm_client: LlmClient) -> Self {
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
