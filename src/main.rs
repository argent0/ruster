mod config;
mod logging;
mod session;
mod llm;
mod skills;
mod server;
mod proactive;
mod servers;

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::signal;
use crate::config::load_config;
use crate::logging::init_logging;
use crate::session::SessionManager;
use crate::skills::SkillsManager;
use crate::llm::LlmClient;
use crate::servers::ServerRegistry;
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

    // 3. Init Servers Registry
    let server_registry = {
        let log_dir = crate::logging::get_log_dir()?;
        let config_dir = log_dir.parent().unwrap().join("config");
        Arc::new(ServerRegistry::new(&config_dir))
    };

    // 4. Init Skills
    let mut skills_manager = SkillsManager::new();
    let _ = skills_manager.ensure_default_skills();
    {
        let cfg = config_arc.read().await;
        skills_manager.load_from_dirs(&cfg.skills_dirs)?;
    }
    let skills_arc = Arc::new(RwLock::new(skills_manager));

    // 5. Init LLM Client
    let llm_client = {
        let cfg = config_arc.read().await;
        LlmClient::new(cfg.proxy_url.clone().unwrap_or("http://localhost:8080".to_string()))
    };

    // 6. Init Session Manager
    let session_manager = Arc::new(SessionManager::new(
        config_arc.clone(),
        skills_arc.clone(),
        llm_client.clone(),
        server_registry.clone(),
    ));

    // 7. Start Discovery Loop
    let sr_clone = server_registry.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = sr_clone.scan_and_update().await {
                tracing::error!("Server discovery error: {}", e);
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        }
    });

    // 8. Start Proactive Loop
    let sm_clone = session_manager.clone();
    let config_clone = config_arc.clone();
    tokio::spawn(async move {
        proactive::start_proactive_loop(sm_clone, config_clone).await;
    });

    // 9. Start Server
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

    // 10. Wait for signals
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;
    
    tokio::select! {
        _ = signal::ctrl_c() => {
            tracing::info!("Received SIGINT, shutting down...");
        }
        _ = sigterm.recv() => {
            tracing::info!("Received SIGTERM, shutting down...");
        }
    }

    // Graceful shutdown logic
    tracing::info!("Saving server registry...");
    let _ = server_registry.save().await;
    
    {
        let cfg = config_arc.read().await;
        let _ = std::fs::remove_file(&cfg.socket_path);
        
        // Remove tool_run_dir
        let tool_run_path = crate::config::expand_path(&cfg.tool_run_dir);
        if tool_run_path.exists() {
            tracing::info!("Removing tool run directory: {:?}", tool_run_path);
            let _ = std::fs::remove_dir_all(&tool_run_path);
        }
    }
    tracing::info!("Shutdown complete.");

    Ok(())
}
