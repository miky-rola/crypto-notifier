use actix_web::{web, App, HttpServer, middleware::Logger};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber;

mod config;
mod state;
mod ws;
mod api;
mod fetcher;

use config::AppConfig;
use state::AppState;
use fetcher::start_price_fetcher;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("asset_price_notifier=debug,actix_web=info")
        .init();

    dotenv::dotenv().ok();
    let config: AppConfig = AppConfig::from_env();
    
    info!("Starting asset price notifier on {}:{}", config.host, config.port);
    info!("Supported crypto tickers: {:?}", config.supported_crypto_tickers);
    info!("Supported stock tickers: {:?}", config.supported_stock_tickers);
    info!("Poll interval: {}s", config.poll_interval_secs);

    let app_state: Arc<AppState> = Arc::new(AppState::new());

    let fetcher_state: Arc<AppState> = Arc::clone(&app_state);
    let fetcher_config: AppConfig = config.clone();
    tokio::spawn(async move {
        start_price_fetcher(fetcher_state, fetcher_config).await;
    });

    let bind_address: String = format!("{}:{}", config.host, config.port);
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(Arc::clone(&app_state)))
            .app_data(web::Data::new(config.clone()))
            .wrap(Logger::default())
            .service(
                web::scope("/api")
                    .route("/price/{ticker}", web::get().to(api::get_price))
                    .route("/tickers", web::get().to(api::get_tickers))
            )
            .route("/ws", web::get().to(ws::websocket_handler))
            .route("/", web::get().to(api::health_check))
    })
    .bind(bind_address)?
    .run()
    .await
}