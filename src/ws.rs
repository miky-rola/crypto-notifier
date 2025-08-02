use actix::{Actor, ActorContext, ActorFutureExt, AsyncContext, Handler, Message, StreamHandler, WrapFuture};
use actix_web::{web, HttpRequest, HttpResponse, Result};
use actix_web_actors::ws;
use serde::{Deserialize, Serialize};
use serde_json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn, error};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::state::{AppState, ClientConnection, PriceData};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Serialize, Deserialize)]
struct SubscriptionRequest {
    tickers: Option<Vec<String>>, // None means all tickers
}

pub struct WsActor {
    pub id: Uuid,
    pub tickers: Vec<String>,
    pub hb: Instant,
    pub app_state: Arc<AppState>,
    pub config: Arc<AppConfig>,
}

impl WsActor {
    pub fn new(app_state: Arc<AppState>, config: Arc<AppConfig>) -> Self {
        let tickers = config.all_supported_tickers(); // Default to all tickers
        Self {
            id: Uuid::new_v4(),
            tickers,
            hb: Instant::now(),
            app_state,
            config,
        }
    }

    fn hb(&self, ctx: &mut <Self as Actor>::Context) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act: &mut WsActor, ctx: &mut ws::WebsocketContext<WsActor>| {
            if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
                warn!("WebSocket client {} heartbeat failed, disconnecting", act.id);
                ctx.stop();
                return;
            }
            ctx.ping(b"");
        });
    }

    async fn send_current_prices(&self, ctx: &mut ws::WebsocketContext<Self>) {
        for ticker in &self.tickers {
            if let Some(price_data) = self.app_state.get_price(ticker) {
                match serde_json::to_string(&price_data) {
                    Ok(json) => ctx.text(json),
                    Err(e) => error!("Failed to serialize price data for {}: {}", ticker, e),
                }
            }
        }
    }

    fn update_subscriptions(&mut self, tickers: Option<Vec<String>>) {
        let new_tickers = match tickers {
            Some(tickers) => {
                let all_supported = self.config.all_supported_tickers();
                tickers
                    .into_iter()
                    .filter(|t| all_supported.contains(&t.to_lowercase()) || all_supported.contains(&t.to_uppercase()))
                    .map(|t| if all_supported.contains(&t.to_uppercase()) { t.to_uppercase() } else { t.to_lowercase() })
                    .collect()
            }
            None => self.config.all_supported_tickers(),
        };
        self.tickers = new_tickers;
    }
}

impl Actor for WsActor {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("WebSocket connection {} started for tickers: {:?}", self.id, self.tickers);
        
        self.hb(ctx);
        
        let connection = ClientConnection {
            id: self.id,
            tickers: self.tickers.clone(),
            addr: ctx.address(),
        };
        
        let app_state = Arc::clone(&self.app_state);
        ctx.spawn(
            async move {
                app_state.add_connection(connection).await;
            }
            .into_actor(self)
        );

        let tickers = self.tickers.clone();
        let app_state = Arc::clone(&self.app_state);
        ctx.spawn(
            async move {
                for ticker in &tickers {
                    if let Some(price_data) = app_state.get_price(ticker) {
                        match serde_json::to_string(&price_data) {
                            Ok(json) => return json,
                            Err(e) => error!("Failed to serialize price data for {}: {}", ticker, e),
                        }
                    }
                }
                String::new()
            }
            .into_actor(self)
            .map(|json, _act, ctx| {
                if !json.is_empty() {
                    ctx.text(json);
                }
            })
        );
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("WebSocket connection {} stopped", self.id);
        
        let app_state = Arc::clone(&self.app_state);
        let connection_id = self.id;
        
        tokio::spawn(async move {
            app_state.remove_connection(connection_id).await;
        });
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct PriceUpdate(pub PriceData);

impl Handler<PriceUpdate> for WsActor {
    type Result = ();

    fn handle(&mut self, msg: PriceUpdate, ctx: &mut Self::Context) {
        if self.tickers.contains(&msg.0.ticker) {
            match serde_json::to_string(&msg.0) {
                Ok(json) => {
                    debug!("Sending price update to client {}: {}", self.id, json);
                    ctx.text(json);
                }
                Err(e) => error!("Failed to serialize price update: {}", e),
            }
        }
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsActor {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.hb = Instant::now();
            }
            Ok(ws::Message::Text(text)) => {
                debug!("Received text message from client {}: {}", self.id, text);
                match serde_json::from_str::<SubscriptionRequest>(&text) {
                    Ok(sub_request) => {
                        self.update_subscriptions(sub_request.tickers);
                        let app_state = Arc::clone(&self.app_state);
                        let tickers = self.tickers.clone();
                        let connection_id = self.id;
                        let addr = ctx.address();
                        ctx.spawn(
                            async move {
                                app_state.remove_connection(connection_id).await;
                                app_state.add_connection(ClientConnection {
                                    id: connection_id,
                                    tickers,
                                    addr,
                                }).await;
                            }
                            .into_actor(self)
                        );
                        let app_state = Arc::clone(&self.app_state);
                        let tickers = self.tickers.clone();
                        ctx.spawn(
                            async move {
                                for ticker in &tickers {
                                    if let Some(price_data) = app_state.get_price(&ticker) {
                                        match serde_json::to_string(&price_data) {
                                            Ok(json) => return json,
                                            Err(e) => error!("Failed to serialize price data for {}: {}", ticker, e),
                                        }
                                    }
                                }
                                String::new()
                            }
                            .into_actor(self)
                            .map(|json, _act, ctx| {
                                if !json.is_empty() {
                                    ctx.text(json);
                                }
                            })
                        );
                    }
                    Err(e) => error!("Failed to parse subscription request: {}", e),
                }
            }
            Ok(ws::Message::Binary(_)) => {
                warn!("Binary messages not supported");
            }
            Ok(ws::Message::Close(reason)) => {
                info!("WebSocket connection {} closed: {:?}", self.id, reason);
                ctx.stop();
            }
            Err(e) => {
                error!("WebSocket protocol error: {}", e);
                ctx.stop();
            }
            _ => {}
        }
    }
}

pub async fn websocket_handler(
    req: HttpRequest,
    stream: web::Payload,
    app_state: web::Data<Arc<AppState>>,
    config: web::Data<AppConfig>,
) -> Result<HttpResponse> {
    info!("New WebSocket connection request");
    
    let max_connections = config.max_connections_per_ticker;
    let all_tickers = config.all_supported_tickers();
    
    for ticker in &all_tickers {
        if app_state.get_connection_count(ticker) >= max_connections {
            return Ok(HttpResponse::TooManyRequests().json(serde_json::json!({
                "error": "Too many connections for ticker",
                "ticker": ticker,
                "current_connections": app_state.get_connection_count(ticker),
                "max_connections": max_connections
            })));
        }
    }
    
    let ws_actor = WsActor::new(app_state.get_ref().clone(), Arc::new(config.get_ref().clone()));
    
    ws::start(ws_actor, &req, stream)
}