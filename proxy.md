### Rust Client Guide for `llm-proxy-rs`

(This section is kept **verbatim** as required.)

This guide explains how to integrate your Rust applications with the `llm-proxy-rs` proxy. The proxy allows you to interact with LLM providers (xAI, Google Gemini, Ollama) without exposing your API keys to the client and while centralizing authentication.

#### Table of Contents
1. [Base URLs & Routing](#base-urls--routing)
2. [Proxy Authentication](#proxy-authentication)
3. [Provider Specific Examples](#provider-specific-examples)
    - [xAI (using `async-openai`)](#xai-using-async-openai)
    - [Google Gemini (using `reqwest`)](#google-gemini-using-reqwest)
    - [Ollama (using `reqwest`)](#ollama-using-reqwest)
4. [Streaming Responses](#streaming-responses)

#### Base URLs & Routing

The proxy routes requests based on the URL path prefix:

| Provider | Proxy Prefix | Target Backend |
|----------|--------------|----------------|
| **xAI** | `/xai` | `https://api.x.ai` |
| **Gemini** | `/gemini` | `https://generativelanguage.googleapis.com` |
| **Ollama** | `/ollama` | *Configured Ollama URL* |

**Example:**  
If your proxy is running at `http://localhost:8080`, you would use:  
- `http://localhost:8080/xai/v1/chat/completions` for xAI chat completions.  
- `http://localhost:8080/gemini/v1beta/models/gemini-1.5-pro:generateContent` for Gemini content generation.

#### Proxy Authentication

If the proxy is configured with `PROXY_AUTH_TOKEN`, you must include it in every request using one of the following headers:

1. `X-Proxy-Auth: <your-token>`  
2. `Authorization: Bearer <your-token>`

**Note:** You do *not* need to provide the provider's API keys (like xAI or Gemini keys); the proxy injects them automatically.

#### Provider Specific Examples

##### xAI (using `async-openai`)

Since xAI provides an OpenAI-compatible API, you can use the `async-openai` crate.

```rust
use async_openai::{
    config::OpenAIConfig,
    types::{CreateChatCompletionRequestArgs, ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs},
    Client,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = OpenAIConfig::new()
        .with_api_base("http://localhost:8080/xai/v1")
        .with_api_key("your-proxy-auth-token");

    let client = Client::with_config(config);

    let request = CreateChatCompletionRequestArgs::default()
        .max_tokens(512u32)
        .model("grok-beta")
        .messages([
            ChatCompletionRequestSystemMessageArgs::default()
                .content("You are a helpful assistant.")
                .build()?
                .into(),
            ChatCompletionRequestUserMessageArgs::default()
                .content("Explain quantum entanglement in one sentence.")
                .build()?
                .into(),
        ])
        .build()?;

    let response = client.chat().create(request).await?;

    println!("Response: {}", response.choices[0].message.content.as_ref().unwrap());

    Ok(())
}
```

##### Google Gemini (using `reqwest`)

```rust
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let proxy_url = "http://localhost:8080/gemini/v1beta/models/gemini-1.5-flash:generateContent";
    let proxy_token = "your-proxy-auth-token";

    let payload = json!({
        "contents": [{
            "parts": [{"text": "Write a haiku about Rust programming."}]
        }]
    });

    let res = client.post(proxy_url)
        .header("X-Proxy-Auth", proxy_token)
        .json(&payload)
        .send()
        .await?;

    let body: serde_json::Value = res.json().await?;
    println!("Gemini Response: {}", body);

    Ok(())
}
```

##### Ollama (using `reqwest`)

```rust
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let proxy_url = "http://localhost:8080/ollama/api/generate";

    let payload = json!({
        "model": "llama3",
        "prompt": "Why is the sky blue?",
        "stream": false
    });

    let res = client.post(proxy_url)
        .header("X-Proxy-Auth", "your-proxy-auth-token")
        .json(&payload)
        .send()
        .await?;

    let body: serde_json::Value = res.json().await?;
    println!("Ollama Response: {}", body["response"]);

    Ok(())
}
```

#### Streaming Responses

The proxy supports full streaming (Server-Sent Events). When using `reqwest`, you can handle the response as a byte stream:

```rust
use futures_util::StreamExt;

// ... inside async main ...
let mut stream = client.post(proxy_url)
    .header("X-Proxy-Auth", proxy_token)
    .json(&payload)
    .send()
    .await?
    .bytes_stream();

while let Some(item) = stream.next().await {
    let chunk = item?;
    println!("Chunk: {:?}", chunk);
}
```

For `async-openai`, streaming works out of the box when using `.chat().create_stream(request)`.


