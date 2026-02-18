use std::path::{Path, PathBuf};
use std::fs;
use serde::{Deserialize, Serialize};
use anyhow::Result;
use glob::glob;
use crate::llm::LlmClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
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
}

impl SkillsManager {
    pub fn new() -> Self {
        Self { skills: Vec::new() }
    }

    pub fn ensure_default_skills(&self) -> Result<()> {
        let config_dir = crate::config::get_config_dir()?;
        let skills_dir = config_dir.join("skills");
        
        if !skills_dir.exists() {
            fs::create_dir_all(&skills_dir)?;
            
            // Create example skill: joke-teller
            let joke_dir = skills_dir.join("joke-teller");
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
        Ok(())
    }

    pub fn load_from_dirs(&mut self, dirs: &[String]) -> Result<()> {
        for dir_str in dirs {
            let expanded = crate::config::expand_path(dir_str);
            let pattern = expanded.join("*").join("SKILL.md");
            
            if let Some(p_str) = pattern.to_str() {
                for entry in glob(p_str)? {
                    match entry {
                        Ok(path) => {
                            if let Err(e) = self.load_skill(&path) {
                                tracing::warn!("Failed to load skill at {:?}: {}", path, e);
                            }
                        },
                        Err(e) => tracing::warn!("Glob error: {}", e),
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

                let metadata: SkillMetadata = serde_yaml::from_str(frontmatter)?;
                
                if let Some(parent) = path.parent() {
                    if let Some(dir_name) = parent.file_name() {
                        if dir_name.to_string_lossy() != metadata.name {
                             tracing::warn!("Skill folder name {:?} does not match metadata name '{}'", dir_name, metadata.name);
                        }
                    }
                }

                self.skills.push(Skill {
                    path: path.parent().unwrap().to_path_buf(),
                    metadata,
                    instructions: body.trim().to_string(),
                });
                return Ok(());
            }
        }
        
        Err(anyhow::anyhow!("Invalid SKILL.md format: missing YAML frontmatter"))
    }

    pub async fn select_skills(&self, _message: &str, _llm: &LlmClient) -> Result<Vec<Skill>> {
        if self.skills.is_empty() {
            return Ok(Vec::new());
        }

        let mut prompt = String::from("Available skills:\n");
        for skill in &self.skills {
            prompt.push_str(&format!("- {}: {}\n", skill.metadata.name, skill.metadata.description));
        }
        
        prompt.push_str(r#"
User message: ""#);
        prompt.push_str(_message);
        prompt.push_str(r#""

Return a JSON list of skill names that are relevant to this message. If none, return []. Example: ["email_fetch"]"#);

        let mut relevant = Vec::new();
        for skill in &self.skills {
            if _message.to_lowercase().contains(&skill.metadata.name) {
                relevant.push(skill.clone());
            }
        }
        Ok(relevant)
    }
}
