use reqwest::Client;
use serde_json::json;
use futures_util::StreamExt;
use anyhow::{Result, anyhow};
use std::pin::Pin;
use futures_util::Stream;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value, // JSON Schema
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LlmResponse {
    Text(String),
    ToolCall(ToolCall),
}

#[derive(Clone)]
pub struct LlmClient {
    client: Client,
    base_url: String, // e.g. "http://localhost:8080"
}

impl LlmClient {
    pub fn new(base_url: String) -> Self {
        Self {
            client: Client::new(),
            base_url,
        }
    }

    pub async fn embeddings(&self, model_str: &str, input: &str) -> Result<Vec<f32>> {
        let (provider, model_name) = model_str.split_once('/')
            .ok_or_else(|| anyhow!("Invalid model format. Expected 'provider/model'"))?;

        let url = match provider {
            "ollama" => format!("{}/ollama/api/embeddings", self.base_url),
            _ => return Err(anyhow!("Embeddings not implemented for provider: {}", provider)),
        };

        let payload = match provider {
            "ollama" => {
                json!({
                    "model": model_name,
                    "prompt": input
                })
            },
            _ => unreachable!(),
        };

        let res = self.client.post(&url)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            return Err(anyhow!("Embeddings request failed: {} - {}", url, error_text));
        }

        let body: serde_json::Value = res.json().await?;
        
        match provider {
            "ollama" => {
                if let Some(embedding) = body["embedding"].as_array() {
                    let vec: Vec<f32> = embedding.iter()
                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                        .collect();
                    Ok(vec)
                } else {
                    Err(anyhow!("Invalid response from Ollama embeddings: {}", body))
                }
            },
            _ => unreachable!(),
        }
    }

    pub async fn chat_stream(
        &self,
        model_str: &str,
        messages: Vec<serde_json::Value>,
        tools: Option<Vec<Tool>>,
        _system_prompt: Option<String>, 
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmResponse>> + Send>>> {
        // model_str format: "provider/model_name"
        let (provider, model_name) = model_str.split_once('/')
            .ok_or_else(|| anyhow!("Invalid model format. Expected 'provider/model'"))?;

        // Construct URL based on proxy.md
        let url = match provider {
            "ollama" => format!("{}/ollama/api/chat", self.base_url),
            "xai" => format!("{}/xai/v1/chat/completions", self.base_url),
            "gemini" => format!("{}/gemini/v1beta/models/{}:streamGenerateContent", self.base_url, model_name),
            _ => return Err(anyhow!("Unknown provider: {}", provider)),
        };

        // Construct payload
        let payload = match provider {
            "ollama" => {
                // Ollama format
                let mut p = json!({
                    "model": model_name,
                    "messages": messages,
                    "stream": true
                });
                if let Some(t) = tools {
                    let ollama_tools: Vec<_> = t.into_iter().map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.parameters
                            }
                        })
                    }).collect();
                    p["tools"] = json!(ollama_tools);
                }
                p
            },
            "xai" => {
                // OpenAI compatible format
                let mut p = json!({
                    "model": model_name,
                    "messages": messages,
                    "stream": true
                });
                if let Some(t) = tools {
                    let xai_tools: Vec<_> = t.into_iter().map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.parameters
                            }
                        })
                    }).collect();
                    p["tools"] = json!(xai_tools);
                }
                p
            },
            "gemini" => {
                // Gemini format
                let contents: Vec<serde_json::Value> = messages.iter().map(|m| {
                    let role = m["role"].as_str().unwrap_or("user");
                    let text = m["content"].as_str().unwrap_or("");
                    let parts = vec![json!({"text": text})];
                    json!({
                        "role": if role == "assistant" { "model" } else { "user" },
                        "parts": parts
                    })
                }).collect();

                let mut p = json!({
                    "contents": contents,
                });

                if let Some(t) = tools {
                    let gemini_tools: Vec<_> = t.into_iter().map(|tool| {
                        json!({
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters
                        })
                    }).collect();
                    p["tools"] = json!([{ "function_declarations": gemini_tools }]);
                }
                p
            },
            _ => unreachable!(),
        };

        let req = self.client.post(&url)
            .json(&payload);

        let res = req.send().await?;
        
        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            return Err(anyhow!("LLM request failed: {} - {}", url, error_text));
        }

        let stream = res.bytes_stream();

        let provider = provider.to_string();

        let mapped_stream = stream.map(move |item| {
            let chunk = item.map_err(|e| anyhow!("Stream error: {}", e))?;
            let text = std::str::from_utf8(&chunk)?.to_string();
            
            parse_chunk(&provider, &text)
        });

        Ok(Box::pin(mapped_stream))
    }
}

fn parse_chunk(provider: &str, text: &str) -> Result<LlmResponse> {
    match provider {
        "ollama" => {
            let obj: serde_json::Value = serde_json::from_str(text)
                .map_err(|e| anyhow!("Failed to parse Ollama chunk: {} | Text: {}", e, text))?;
            
            if let Some(tool_calls) = obj["message"]["tool_calls"].as_array() {
                if !tool_calls.is_empty() {
                    let tc = &tool_calls[0]["function"];
                    return Ok(LlmResponse::ToolCall(ToolCall {
                        id: "ollama".to_string(), // Ollama doesn't always provide IDs in chunks?
                        name: tc["name"].as_str().unwrap_or_default().to_string(),
                        arguments: tc["arguments"].to_string(),
                    }));
                }
            }

            if let Some(content) = obj["message"]["content"].as_str() {
                Ok(LlmResponse::Text(content.to_string()))
            } else {
                Ok(LlmResponse::Text("".to_string()))
            }
        },
        "xai" => {
             let mut content = String::new();
             for line in text.lines() {
                 if line.starts_with("data: ") {
                     let json_str = &line[6..];
                     if json_str == "[DONE]" { continue; }
                     if let Ok(obj) = serde_json::from_str::<serde_json::Value>(json_str) {
                         if let Some(tool_calls) = obj["choices"][0]["delta"]["tool_calls"].as_array() {
                             if !tool_calls.is_empty() {
                                 let tc = &tool_calls[0];
                                 let func = &tc["function"];
                                 return Ok(LlmResponse::ToolCall(ToolCall {
                                     id: tc["id"].as_str().unwrap_or_default().to_string(),
                                     name: func["name"].as_str().unwrap_or_default().to_string(),
                                     arguments: func["arguments"].as_str().unwrap_or_default().to_string(),
                                 }));
                             }
                         }

                         if let Some(c) = obj["choices"][0]["delta"]["content"].as_str() {
                             content.push_str(c);
                         }
                     }
                 }
             }
             Ok(LlmResponse::Text(content))
        },
        "gemini" => {
            let text_trimmed = text.trim().trim_start_matches(',').trim();
             if let Ok(obj) = serde_json::from_str::<serde_json::Value>(text_trimmed) {
                 if let Some(parts) = obj["candidates"][0]["content"]["parts"].as_array() {
                     for part in parts {
                         if let Some(call) = part["functionCall"].as_object() {
                             return Ok(LlmResponse::ToolCall(ToolCall {
                                 id: "gemini".to_string(),
                                 name: call["name"].as_str().unwrap_or_default().to_string(),
                                 arguments: call["args"].to_string(),
                             }));
                         }
                         if let Some(t) = part["text"].as_str() {
                             return Ok(LlmResponse::Text(t.to_string()));
                         }
                     }
                 }
             }
             Ok(LlmResponse::Text("".to_string()))
        },
        _ => Ok(LlmResponse::Text("".to_string()))
    }
}

