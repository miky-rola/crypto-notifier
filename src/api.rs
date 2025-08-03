use actix_web::{web, HttpResponse, Result};
use serde_json;
use std::sync::Arc;
use tracing::info;

use crate::config::AppConfig;
use crate::state::AppState;

pub async fn health_check() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "status": "healthy",
        "service": "asset-price-notifier"
    })))
}

pub async fn get_price(
    path: web::Path<String>,
    app_state: web::Data<Arc<AppState>>,
    config: web::Data<AppConfig>,
) -> Result<HttpResponse> {
    let ticker: String = path.into_inner();
    let ticker_lower: String = ticker.to_lowercase();
    let ticker_upper: String = ticker.to_uppercase();
    
    if !config.all_supported_tickers().contains(&ticker_lower) && !config.all_supported_tickers().contains(&ticker_upper) {
        return Ok(HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Unsupported ticker",
            "supported_tickers": config.all_supported_tickers()
        })));
    }
    
    let ticker: String = if config.supported_crypto_tickers.contains(&ticker_lower) {
        ticker_lower
    } else {
        ticker_upper
    };
    
    match app_state.get_price(&ticker) {
        Some(price_data) => {
            info!("Returning price for ticker: {}", ticker);
            Ok(HttpResponse::Ok().json(price_data))
        }
        None => {
            Ok(HttpResponse::NotFound().json(serde_json::json!({
                "error": "Price data not available for this ticker",
                "ticker": ticker
            })))
        }
    }
}

pub async fn get_tickers(
    app_state: web::Data<Arc<AppState>>,
    config: web::Data<AppConfig>,
) -> Result<HttpResponse> {
    let available_tickers: Vec<String> = app_state.get_all_tickers();
    
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "supported_crypto_tickers": config.supported_crypto_tickers,
        "supported_stock_tickers": config.supported_stock_tickers,
        "available_tickers": available_tickers,
        "total_supported": config.all_supported_tickers().len(),
        "total_available": available_tickers.len()
    })))
}