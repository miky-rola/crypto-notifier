use std::env;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub supported_crypto_tickers: Vec<String>,
    pub supported_stock_tickers: Vec<String>,
    pub poll_interval_secs: u64,
    pub coingecko_api_url: String,
    pub stock_api_url: String, 
    pub max_connections_per_ticker: usize,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            host: env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            port: env::var("PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .expect("PORT must be a valid number"),
            supported_crypto_tickers: env::var("SUPPORTED_CRYPTO_TICKERS")
                .unwrap_or_else(|_| "bitcoin,ethereum,cardano,polkadot,chainlink".to_string())
                .split(',')
                .map(|s: &str| s.trim().to_lowercase())
                .collect(),
            supported_stock_tickers: env::var("SUPPORTED_STOCK_TICKERS")
                .unwrap_or_else(|_| "AAPL,GOOGL,MSFT,AMZN,TSLA".to_string())
                .split(',')
                .map(|s: &str| s.trim().to_uppercase())
                .collect(),
            poll_interval_secs: env::var("POLL_INTERVAL_SECS")
                .unwrap_or_else(|_| "30".to_string())
                .parse()
                .expect("POLL_INTERVAL_SECS must be a valid number"),
            coingecko_api_url: env::var("COINGECKO_API_URL")
                .unwrap_or_else(|_| "https://api.coingecko.com/api/v3".to_string()),
            stock_api_url: env::var("STOCK_API_URL")
                .unwrap_or_else(|_| "https://www.alphavantage.co".to_string()),
            max_connections_per_ticker: env::var("MAX_CONNECTIONS_PER_TICKER")
                .unwrap_or_else(|_| "100".to_string())
                .parse()
                .expect("MAX_CONNECTIONS_PER_TICKER must be a valid number"),
        }
    }

    pub fn all_supported_tickers(&self) -> Vec<String> {
        let mut all_tickers: Vec<String> = self.supported_crypto_tickers.clone();
        all_tickers.extend(self.supported_stock_tickers.clone());
        all_tickers
    }
}