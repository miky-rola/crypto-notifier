use reqwest;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::config::AppConfig;
use crate::state::{AppState, PriceData};
use crate::ws::PriceUpdate;

#[derive(Debug, Deserialize)]
struct CoinGeckoResponse {
    #[serde(flatten)]
    prices: HashMap<String, CoinGeckoPrice>,
}

#[derive(Debug, Deserialize)]
struct CoinGeckoPrice {
    usd: f64,
    usd_24h_change: f64,
}

#[derive(Debug, Deserialize)]
struct StockResponse {
    #[serde(rename = "Global Quote")]
    global_quote: StockPrice,
}

#[derive(Debug, Deserialize)]
struct StockPrice {
    #[serde(rename = "05. price")]
    price: String,
    #[serde(rename = "10. change percent")]
    change_percent: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("JSON parsing failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("API response format error: {0}")]
    Format(String),
}

async fn fetch_crypto_prices(
    client: &reqwest::Client,
    config: &AppConfig,
) -> Result<Vec<PriceData>, FetchError> {
    let tickers_param: String = config.supported_crypto_tickers.join(",");
    let url: String = format!(
        "{}/simple/price?ids={}&vs_currencies=usd&include_24hr_change=true",
        config.coingecko_api_url, tickers_param
    );

    debug!("Fetching crypto prices from: {}", url);

    let response: reqwest::Response = client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(FetchError::Format(format!(
            "CoinGecko API returned status: {}",
            response.status()
        )));
    }

    let body: String = response.text().await?;
    debug!("CoinGecko API response: {}", body);

    let parsed: CoinGeckoResponse = serde_json::from_str(&body)?;

    let mut price_data_list: Vec<PriceData> = Vec::new();
    let now: String = chrono::Utc::now().to_rfc3339();

    for (ticker, price_info) in parsed.prices {
        let price_data: PriceData = PriceData {
            ticker: ticker.clone(),
            asset_type: "crypto".to_string(),
            price_usd: price_info.usd,
            change_24h: price_info.usd_24h_change,
            last_updated: now.clone(),
        };
        price_data_list.push(price_data);
    }

    Ok(price_data_list)
}

async fn fetch_stock_prices(
    client: &reqwest::Client,
    config: &AppConfig,
) -> Result<Vec<PriceData>, FetchError> {
    let mut price_data_list: Vec<PriceData> = Vec::new();
    let now: String = chrono::Utc::now().to_rfc3339();

    for ticker in &config.supported_stock_tickers {
        let url: String = format!(
            "{}/query?function=GLOBAL_QUOTE&symbol={}&apikey={}",
            config.stock_api_url, ticker, "demo" // Replace with actual API key in production
        );

        debug!("Fetching stock price for {} from: {}", ticker, url);

        let response: reqwest::Response = client
            .get(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;

        if !response.status().is_success() {
            warn!("Stock API returned status {} for {}", response.status(), ticker);
            continue;
        }

        let body: String = response.text().await?;
        debug!("Stock API response for {}: {}", ticker, body);

        let parsed: StockResponse = serde_json::from_str(&body)?;
        let price: f64 = parsed.global_quote.price.parse().map_err(|_| {
            FetchError::Format(format!("Invalid price format for {}", ticker))
        })?;
        let change_percent: f64 = parsed.global_quote.change_percent.trim_end_matches('%').parse().map_err(|_| {
            FetchError::Format(format!("Invalid change percent format for {}", ticker))
        })?;

        let price_data: PriceData = PriceData {
            ticker: ticker.clone(),
            asset_type: "stock".to_string(),
            price_usd: price,
            change_24h: change_percent,
            last_updated: now.clone(),
        };
        price_data_list.push(price_data);
    }

    Ok(price_data_list)
}

async fn send_price_updates(app_state: &Arc<AppState>, price_data: &PriceData) {
    let connections: tokio::sync::RwLockReadGuard<'_, HashMap<uuid::Uuid, crate::state::ClientConnection>> = app_state.connections.read().await;
    
    for (conn_id, connection) in connections.iter() {
        if connection.tickers.contains(&price_data.ticker) {
            let update_msg = PriceUpdate(price_data.clone());
            if let Err(_) = connection.addr.try_send(update_msg) {
                warn!("Dead connection detected for client {}: {}", price_data.ticker, conn_id);
                let app_state = Arc::clone(app_state);
                let conn_id = *conn_id;
                tokio::spawn(async move {
                    app_state.remove_connection(conn_id).await;
                });
            } else {
                debug!("Sent price update to connection {}", conn_id);
            }
        }
    }
}

pub async fn start_price_fetcher(app_state: Arc<AppState>, config: AppConfig) {
    info!("Starting price fetcher with interval: {}s", config.poll_interval_secs);
    
    let client = reqwest::Client::builder()
        .user_agent("asset-price-notifier/1.0")
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client");

    let mut interval: time::Interval = time::interval(Duration::from_secs(config.poll_interval_secs));
    
    loop {
        interval.tick().await;
        
        debug!("Fetching price updates...");
        
        // Fetch crypto prices
        match fetch_crypto_prices(&client, &config).await {
            Ok(price_data_list) => {
                info!("Successfully fetched {} crypto price updates", price_data_list.len());
                for price_data in price_data_list {
                    let should_update: bool = if let Some(existing) = app_state.get_price(&price_data.ticker) {
                        let price_diff: f64 = (price_data.price_usd - existing.price_usd).abs();
                        let change_threshold: f64 = existing.price_usd * 0.001;
                        price_diff > change_threshold
                    } else {
                        true
                    };
                    
                    if should_update {
                        info!(
                            "Price update for {} ({}): ${:.4} ({:+.2}%)",
                            price_data.ticker, price_data.asset_type, price_data.price_usd, price_data.change_24h
                        );
                        app_state.update_price(price_data.ticker.clone(), price_data.clone());
                        send_price_updates(&app_state, &price_data).await;
                    } else {
                        debug!("No significant price change for {}, skipping update", price_data.ticker);
                    }
                }
            }
            Err(e) => error!("Failed to fetch crypto prices: {}", e),
        }

        // Fetch stock prices
        match fetch_stock_prices(&client, &config).await {
            Ok(price_data_list) => {
                info!("Successfully fetched {} stock price updates", price_data_list.len());
                for price_data in price_data_list {
                    let should_update = if let Some(existing) = app_state.get_price(&price_data.ticker) {
                        let price_diff = (price_data.price_usd - existing.price_usd).abs();
                        let change_threshold = existing.price_usd * 0.001;
                        price_diff > change_threshold
                    } else {
                        true
                    };
                    
                    if should_update {
                        info!(
                            "Price update for {} ({}): ${:.4} ({:+.2}%)",
                            price_data.ticker, price_data.asset_type, price_data.price_usd, price_data.change_24h
                        );
                        app_state.update_price(price_data.ticker.clone(), price_data.clone());
                        send_price_updates(&app_state, &price_data).await;
                    } else {
                        debug!("No significant price change for {}, skipping update", price_data.ticker);
                    }
                }
            }
            Err(e) => error!("Failed to fetch stock prices: {}", e),
        }
    }
}