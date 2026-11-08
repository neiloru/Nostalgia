//! Server-Sent-Events hub used to push new cloud saves / library changes to open clients.

use nostalgia_shared::ServerEvent;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc::UnboundedSender;

struct Client {
    id: u64,
    user_id: i64,
    device: Option<String>,
    tx: UnboundedSender<ServerEvent>,
}

#[derive(Default)]
pub struct EventHub {
    clients: Mutex<Vec<Client>>,
    next: AtomicU64,
}

impl EventHub {
    /// Register a connection; returns its id for `remove`.
    pub fn add(&self, user_id: i64, device: Option<String>, tx: UnboundedSender<ServerEvent>) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let _ = tx.send(ServerEvent::Hello { server_time: nostalgia_shared::iso_now() });
        self.clients.lock().unwrap().push(Client { id, user_id, device, tx });
        id
    }

    pub fn remove(&self, id: u64) {
        self.clients.lock().unwrap().retain(|c| c.id != id);
    }

    pub fn to_user(&self, user_id: i64, ev: ServerEvent, except_device: Option<&str>) {
        let clients = self.clients.lock().unwrap();
        for c in clients.iter() {
            let skip = except_device.is_some_and(|d| !d.is_empty() && c.device.as_deref() == Some(d));
            if c.user_id == user_id && !skip {
                let _ = c.tx.send(ev.clone());
            }
        }
    }

    pub fn to_all(&self, ev: ServerEvent) {
        for c in self.clients.lock().unwrap().iter() {
            let _ = c.tx.send(ev.clone());
        }
    }

    pub fn count(&self) -> usize {
        self.clients.lock().unwrap().len()
    }

    pub fn online_user_ids(&self) -> Vec<i64> {
        let mut ids: Vec<i64> = self.clients.lock().unwrap().iter().map(|c| c.user_id).collect();
        ids.sort();
        ids.dedup();
        ids
    }
}
