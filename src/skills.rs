use std::path::{Path, PathBuf};
use std::fs;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::json;
use anyhow::Result;
use glob::glob;
use crate::llm::LlmClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tools: Vec<crate::llm::Tool>,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub path: PathBuf,
    pub metadata: SkillMetadata,
    pub instructions: String,
}

#[derive(Clone)]
pub struct SkillsManager {
    skills: Vec<Skill>,
    embedding_cache: HashMap<(String, String), Vec<f32>>,
}

impl SkillsManager {
    pub fn new() -> Self {
        Self { 
            skills: Vec::new(),
            embedding_cache: HashMap::new(),
        }
    }

    pub fn ensure_default_skills(&self) -> Result<()> {
        let config_dir = crate::config::get_config_dir()?;
        let skills_dir = config_dir.join("skills");
        
        if !skills_dir.exists() {
            fs::create_dir_all(&skills_dir)?;
        }

        // Add Joke-teller skill
        let joke_dir = skills_dir.join("joke-teller");
        if !joke_dir.exists() {
            fs::create_dir_all(&joke_dir)?;
            let skill_md = r#"---
name: joke-teller
description: Tells funny programming jokes. Use when user asks for a laugh.
---

# Joke Teller Instructions

You are a comedian specialized in programming humor.
When the user asks for a joke, provide one related to:
- Rust borrowing checker
- Python whitespace
- Java verbosity

Keep it short and punchy.
"#;
            fs::write(joke_dir.join("SKILL.md"), skill_md)?;
            tracing::info!("Created default skill at {:?}", joke_dir);
        }

        // Add Clock skill
        let clock_dir = skills_dir.join("clock");
        fs::create_dir_all(&clock_dir)?;
        let clock_md = r#"---
name: clock
description: Fetches current date and time using system tools.
tools:
  - name: get_current_time
    description: Returns the current system time.
    parameters:
      type: object
      properties: {}
    exec: "date '+%A, %B %d, %Y %H:%M:%S %Z'"
---

# Clock Instructions

Whenever the user asks for the current time or date, you MUST use the `get_current_time` tool to fetch the most up-to-date information.
Do not rely on your internal knowledge or the time the message was sent.
"#;
        fs::write(clock_dir.join("SKILL.md"), clock_md)?;
        tracing::info!("Updated default skill at {:?}", clock_dir);

        // Add Skill Manager skill
        let manager_dir = skills_dir.join("skill-manager");
        if !manager_dir.exists() {
            fs::create_dir_all(&manager_dir)?;
            let manager_md = r#"---
name: skill-manager
description: Manage skills in the current session. Use to add, list, search, remove, ban, or unban skills.
---

# Skill Manager Instructions

You are the Skill Manager for this session. You can help the user manage their modular capabilities.
To manage skills, tell the user to use the following commands (or explain them):
- `skill add <name>`: Adds a skill permanently to this session's context.
- `skill list`: Lists all skills currently active in this session.
- `skill search <query>`: Searches for available skills using RAG.
- `skill remove <name>`: Removes a skill from the session and its history.
- `skill ban <name>`: Globally prevents a skill from being loaded or dynamically selected.
- `skill unban <name>`: Removes a skill from the global ban list.

Note: These are system commands that the user should send as structured requests.
If you need a specific capability, you can ask the user to 'add' the relevant skill.
"#;
            fs::write(manager_dir.join("SKILL.md"), manager_md)?;
            tracing::info!("Created default skill at {:?}", manager_dir);
        }

        Ok(())
    }

    pub fn load_from_dirs(&mut self, dirs: &[String]) -> Result<()> {
        tracing::debug!(dirs = ?dirs, "Scanning directories for skills.");
        for dir_str in dirs {
            let expanded = crate::config::expand_path(dir_str);
            let pattern = expanded.join("*").join("SKILL.md");
            
            if let Some(p_str) = pattern.to_str() {
                for entry in glob(p_str)? {
                    match entry {
                        Ok(path) => {
                            if let Err(e) = self.load_skill(&path) {
                                tracing::error!(path = %path.display(), error = %e, "Failed to load skill.");
                            }
                        },
                        Err(e) => tracing::error!(error = %e, "Glob error when scanning for skills."),
                    }
                }
            }
        }
        Ok(())
    }

    fn load_skill(&mut self, path: &Path) -> Result<()> {
        let content = fs::read_to_string(path)?;
        
        if content.starts_with("---") {
            if let Some(end) = content[3..].find("---") {
                let frontmatter = &content[3..3+end];
                let body = &content[3+end+3..];

                let mut metadata: SkillMetadata = serde_yaml::from_str(frontmatter)?;
                
                let skill_path = path.parent().unwrap().to_path_buf();
                
                // Set working_dir for tools if not already set
                for tool in &mut metadata.tools {
                    if tool.exec.is_some() && tool.working_dir.is_none() {
                        tool.working_dir = Some(skill_path.to_string_lossy().to_string());
                    }
                }

                if let Some(dir_name) = path.parent().and_then(|p| p.file_name()) {
                    if dir_name.to_string_lossy() != metadata.name {
                         tracing::warn!(dir_name = %dir_name.to_string_lossy(), metadata_name = %metadata.name, "Skill folder name does not match metadata name.");
                    }
                }

                tracing::info!(skill = %metadata.name, "Loaded skill.");

                self.skills.push(Skill {
                    path: skill_path,
                    metadata,
                    instructions: body.trim().to_string(),
                });

                return Ok(());
            }
        }
        
        tracing::error!(path = %path.display(), "Invalid SKILL.md format: missing YAML frontmatter.");
        Err(anyhow::anyhow!("Invalid SKILL.md format: missing YAML frontmatter"))
    }

    pub fn get_skill(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.metadata.name == name)
    }

    pub fn list_skills(&self) -> Vec<SkillMetadata> {
        self.skills.iter().map(|s| s.metadata.name.clone()).map(|name| {
            // Re-finding to get full metadata easily or just clone it
            self.get_skill(&name).unwrap().metadata.clone()
        }).collect()
    }

    pub async fn search_skills(&mut self, query: &str, llm: &LlmClient, rag_model: &str) -> Result<Vec<Skill>> {
        self.select_skills(query, llm, rag_model).await
    }

    pub async fn select_skills(&mut self, message: &str, llm: &LlmClient, rag_model: &str) -> Result<Vec<Skill>> {
        if self.skills.is_empty() {
            tracing::debug!("No skills available to select from.");
            return Ok(Vec::new());
        }

        tracing::info!(rag_model = %rag_model, message_len = %message.len(), "Starting RAG skill selection");

        // 1. Get embedding for the message
        let query_embedding = match llm.embeddings(rag_model, message).await {
            Ok(emb) => {
                tracing::debug!("Successfully obtained message embedding");
                emb
            },
            Err(e) => {
                tracing::warn!(error = %e, "Failed to get query embedding. Falling back to keyword search.");
                let mut relevant = Vec::new();
                for skill in &self.skills {
                    if message.to_lowercase().contains(&skill.metadata.name.to_lowercase()) {
                        tracing::info!(skill = %skill.metadata.name, "Skill selected via keyword fallback.");
                        relevant.push(skill.clone());
                    }
                }
                return Ok(relevant);
            }
        };

        // 2. Ensure all skills have embeddings cached for this model
        for skill in &self.skills {
            let key = (rag_model.to_string(), skill.metadata.name.clone());
            if !self.embedding_cache.contains_key(&key) {
                let text = format!("{}: {}", skill.metadata.name, skill.metadata.description);
                tracing::debug!(skill = %skill.metadata.name, "Generating embedding for skill...");
                match llm.embeddings(rag_model, &text).await {
                    Ok(emb) => {
                        self.embedding_cache.insert(key, emb);
                    },
                    Err(e) => {
                        tracing::error!(error = %e, skill = %skill.metadata.name, "Failed to get embedding for skill. Skill will be skipped in RAG search.");
                    }
                }
            }
        }

        // 3. Compute cosine similarity
        let mut scores: Vec<(&Skill, f32)> = Vec::new();
        for skill in &self.skills {
            let key = (rag_model.to_string(), skill.metadata.name.clone());
            if let Some(emb) = self.embedding_cache.get(&key) {
                let score = cosine_similarity(&query_embedding, emb);
                tracing::debug!(skill = %skill.metadata.name, score = %score, "Computed similarity score.");
                scores.push((skill, score));
            }
        }

        // 4. Sort and select top skills
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        
        let mut relevant = Vec::new();
        for (skill, score) in scores {
            // Threshold for relevance
            if score > 0.4 {
                 tracing::info!(skill = %skill.metadata.name, score = %score, "Skill selected via RAG.");
                 relevant.push(skill.clone());
            } else {
                 tracing::debug!(skill = %skill.metadata.name, score = %score, "Skill discarded (below threshold).");
            }
        }

        // Limit to top 3 to keep context manageable
        if relevant.len() > 3 {
            tracing::debug!("Truncating relevant skills list to top 3.");
            relevant.truncate(3);
        }

        Ok(relevant)
    }
}

fn cosine_similarity(v1: &[f32], v2: &[f32]) -> f32 {
    if v1.len() != v2.len() || v1.is_empty() {
        return 0.0;
    }
    let dot_product: f32 = v1.iter().zip(v2.iter()).map(|(a, b)| a * b).sum();
    let norm1: f32 = v1.iter().map(|a| a * a).sum::<f32>().sqrt();
    let norm2: f32 = v2.iter().map(|a| a * a).sum::<f32>().sqrt();
    
    if norm1 == 0.0 || norm2 == 0.0 {
        return 0.0;
    }
    
    dot_product / (norm1 * norm2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        let v1 = vec![1.0, 0.0];
        let v2 = vec![1.0, 0.0];
        assert!((cosine_similarity(&v1, &v2) - 1.0).abs() < 1e-6);

        let v3 = vec![0.0, 1.0];
        assert!((cosine_similarity(&v1, &v3) - 0.0).abs() < 1e-6);

        let v4 = vec![-1.0, 0.0];
        assert!((cosine_similarity(&v1, &v4) - (-1.0)).abs() < 1e-6);
    }
}
