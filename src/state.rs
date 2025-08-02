use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceData {
    pub ticker: String,
    pub asset_type: String, // New field: "crypto" or "stock"
    pub price_usd: f64,
    pub change_24h: f64,
    pub last_updated: String,
}

#[derive(Debug, Clone)]
pub struct ClientConnection {
    pub id: Uuid,
    pub tickers: Vec<String>, // Changed to support multiple tickers
    pub addr: actix::Addr<crate::ws::WsActor>,
}

pub struct AppState {
    pub prices: DashMap<String, PriceData>,
    pub connections: RwLock<HashMap<Uuid, ClientConnection>>,
    pub connection_counts: DashMap<String, usize>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            prices: DashMap::new(),
            connections: RwLock::new(HashMap::new()),
            connection_counts: DashMap::new(),
        }
    }

    pub async fn add_connection(&self, connection: ClientConnection) {
        let mut connections = self.connections.write().await;
        connections.insert(connection.id, connection.clone());
        
        for ticker in &connection.tickers {
            let mut count = self.connection_counts.entry(ticker.clone()).or_insert(0);
            *count += 1;
        }
    }

    pub async fn remove_connection(&self, connection_id: Uuid) {
        let mut connections = self.connections.write().await;
        if let Some(connection) = connections.remove(&connection_id) {
            for ticker in connection.tickers {
                if let Some(mut count) = self.connection_counts.get_mut(&ticker) {
                    if *count > 0 {
                        *count -= 1;
                    }
                }
            }
        }
    }

    pub fn get_price(&self, ticker: &str) -> Option<PriceData> {
        self.prices.get(ticker).map(|entry| entry.clone())
    }

    pub fn update_price(&self, ticker: String, price_data: PriceData) {
        self.prices.insert(ticker, price_data);
    }

    pub fn get_all_tickers(&self) -> Vec<String> {
        self.prices.iter().map(|entry| entry.key().clone()).collect()
    }

    pub fn get_connection_count(&self, ticker: &str) -> usize {
        self.connection_counts.get(ticker).map(|count| *count).unwrap_or(0)
    }
}