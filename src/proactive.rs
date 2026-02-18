use std::sync::Arc;
use tokio::time::Duration;
use serde_json::json;
use crate::session::SessionManager;
use crate::config::Config;

pub async fn start_proactive_loop(session_manager: Arc<SessionManager>, config: Config) {
    let interval = Duration::from_secs(config.proactive_interval_secs);
    let mut timer = tokio::time::interval(interval);

    loop {
        timer.tick().await;
        
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
