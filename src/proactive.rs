use std::sync::Arc;
use tokio::time::Duration;
use tokio::sync::RwLock;
use serde_json::json;
use crate::session::SessionManager;
use crate::config::Config;

pub async fn start_proactive_loop(session_manager: Arc<SessionManager>, config: Arc<RwLock<Config>>) {
    let mut interval_secs = {
        let cfg = config.read().await;
        cfg.proactive_interval_secs
    };
    let mut timer = tokio::time::interval(Duration::from_secs(interval_secs));

    loop {
        timer.tick().await;
        
        // Check if interval changed
        let current_interval = {
            let cfg = config.read().await;
            cfg.proactive_interval_secs
        };
        
        if current_interval != interval_secs {
            interval_secs = current_interval;
            timer = tokio::time::interval(Duration::from_secs(interval_secs));
            // Reset the timer to the new interval
            timer.tick().await; // skip immediate tick
        }
        
        // Logic to check internal tasks would go here.
        // For now, we simulate a proactive event.
        // We broadcast to all connected clients.
        // But what session_id?
        // Spec example: {"event":"proactive","session_id":"work","message":"Reminder: meeting in 30 min"}
        // This implies we need to find active sessions and check tasks for them.
        
        // Let's iterate all sessions and pretend we found something for one of them?
        // Or just broadcast a generic system event?
        // If we broadcast with session_id="system", clients might not expect it.
        // Let's just log and skip if no sessions.
        
        let sessions = {
            let map = session_manager.sessions.read().await;
            map.keys().cloned().collect::<Vec<_>>()
        };
        
        if !sessions.is_empty() {
             // Pick one or all?
             // Let's just pick the first one for demonstration.
             if let Some(id) = sessions.first() {
                 let _ = session_manager.event_sender.send(json!({
                     "event": "proactive",
                     "session_id": id,
                     "message": "Proactive check: System operational."
                 }));
             }
        }
        
        tracing::debug!("Proactive loop tick");
    }
}
