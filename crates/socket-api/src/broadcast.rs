use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use uuid::Uuid;

pub struct ConnectionHandle {
    pub tx: mpsc::Sender<String>,
    pub id: Uuid,
}

#[derive(Clone)]
pub struct BroadcastSender {
    connections: Arc<Mutex<Vec<ConnectionHandle>>>,
}

impl BroadcastSender {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn add_connection(&self, tx: mpsc::Sender<String>) -> Uuid {
        let id = Uuid::new_v4();
        self.connections.lock().unwrap().push(ConnectionHandle { tx, id });
        id
    }

    pub fn remove_connection(&self, id: Uuid) {
        self.connections.lock().unwrap().retain(|h| h.id != id);
    }

    pub async fn register_path(&self, path: &str) {
        self.broadcast(format!("REGISTER_PATH:{path}\n")).await;
    }

    pub async fn unregister_path(&self, path: &str) {
        self.broadcast(format!("UNREGISTER_PATH:{path}\n")).await;
    }

    pub async fn status_changed(&self, tag: &str, path: &str) {
        self.broadcast(format!("STATUS:{tag}:{path}\n")).await;
    }

    pub async fn update_view(&self, path: &str) {
        self.broadcast(format!("UPDATE_VIEW:{path}\n")).await;
    }

    async fn broadcast(&self, message: String) {
        let senders: Vec<(Uuid, mpsc::Sender<String>)> = {
            let conns = self.connections.lock().unwrap();
            conns.iter().map(|h| (h.id, h.tx.clone())).collect()
        };

        let mut dead_ids: Vec<Uuid> = Vec::new();

        for (id, tx) in senders {
            if tx.send(message.clone()).await.is_err() {
                dead_ids.push(id);
            }
        }

        if !dead_ids.is_empty() {
            self.connections.lock().unwrap().retain(|h| !dead_ids.contains(&h.id));
        }
    }
}

impl Default for BroadcastSender {
    fn default() -> Self {
        Self::new()
    }
}
