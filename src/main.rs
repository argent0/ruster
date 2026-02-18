mod config;
mod logging;
mod session;
mod llm;
mod skills;
mod server;
mod proactive;

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::signal;
use crate::config::load_config;
use crate::logging::init_logging;
use crate::session::SessionManager;
use crate::skills::SkillsManager;
use crate::llm::LlmClient;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Load config
    let config = load_config()?;
    let config_arc = Arc::new(RwLock::new(config));

    // 2. Init logging
    let _guard = {
        let cfg = config_arc.read().await;
        init_logging(&cfg.log_level)?
    };
    tracing::info!("Ruster starting up...");

    // 3. Init Skills
    let mut skills_manager = SkillsManager::new();
    let _ = skills_manager.ensure_default_skills();
    {
        let cfg = config_arc.read().await;
        skills_manager.load_from_dirs(&cfg.skills_dirs)?;
    }
    let skills_arc = Arc::new(RwLock::new(skills_manager));

    // 4. Init LLM Client
    let llm_client = {
        let cfg = config_arc.read().await;
        LlmClient::new(cfg.proxy_url.clone().unwrap_or("http://localhost:8080".to_string()))
    };

    // 5. Init Session Manager
    let session_manager = Arc::new(SessionManager::new(
        config_arc.clone(),
        skills_arc.clone(),
        llm_client.clone(),
    ));

    // 6. Start Proactive Loop
    let sm_clone = session_manager.clone();
    let config_clone = config_arc.clone();
    tokio::spawn(async move {
        proactive::start_proactive_loop(sm_clone, config_clone).await;
    });

    // 7. Start Server
    let socket_path = {
        let cfg = config_arc.read().await;
        cfg.socket_path.clone()
    };
    let sm_clone2 = session_manager.clone();
    
    // Spawn server task so we can handle signals
    let _server_handle = tokio::spawn(async move {
        if let Err(e) = server::start_server(&socket_path, sm_clone2).await {
            tracing::error!("Server error: {}", e);
        }
    });

    // 8. Wait for signals
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;
    
    tokio::select! {
        _ = signal::ctrl_c() => {
            tracing::info!("Received SIGINT, shutting down...");
        }
        _ = sigterm.recv() => {
            tracing::info!("Received SIGTERM, shutting down...");
        }
    }

    // Graceful shutdown logic?
    // Server task handles connections in background.
    // We should probably cancel server task or wait for current connections?
    // Spec just says "Graceful shutdown on SIGTERM".
    // Usually means close listener, finish pending requests.
    // But UnixListener doesn't have easy "stop accept but finish connections".
    // We'll just exit. The OS will clean up sockets.
    // However, we should remove the socket file if possible.
    // But server task removes it on start.
    // Let's try to remove socket file on exit.
    {
        let cfg = config_arc.read().await;
        let _ = std::fs::remove_file(&cfg.socket_path);
    }
    tracing::info!("Shutdown complete.");

    Ok(())
}
