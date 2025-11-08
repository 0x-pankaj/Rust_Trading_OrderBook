use actix_web::{
    HttpRequest, HttpResponse, Responder, get, post,
    web::{self, Json},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    AppState,
    types::{Order, OrderSide, OrderType, OrderbookCommand},
};

#[get("/get-orderbook")]
pub async fn get_orderbook(data: web::Data<AppState>) -> impl Responder {
    let (tx, rx) = tokio::sync::oneshot::channel();

    let cmd = OrderbookCommand::GetSnapshot { response: tx };

    if data.orderbook_tx.send(cmd).await.is_err() {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": "failed to get orderbook snapshot"
        }));
    }

    match rx.await {
        Ok(snapshot) => HttpResponse::Ok().json(snapshot),
        Err(_) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": "failed to receive orderbook snapshot"
        })),
    }
}

#[derive(Deserialize)]
struct MarketOrderRequest {
    quantity: f64,
    side: String,
}

#[post("/create-market-order")]
async fn create_market_order(
    data: web::Data<AppState>,
    body: Json<MarketOrderRequest>,
    req: HttpRequest,
) -> impl Responder {
    let username = match get_username_from_token(data.clone(), req) {
        Some(u) => u,
        None => {
            return HttpResponse::Unauthorized().json(serde_json::json!({
                "success": false,
                "message": "unauthorized".to_string()
            }));
        }
    };

    let user_id = {
        let users = data.users.lock().unwrap();
        match users.get(&username) {
            Some(u) => u.id.clone(),
            None => {
                return HttpResponse::Unauthorized().json(serde_json::json!({
                    "success": false,
                    "message": "user not found".to_string()
                }));
            }
        }
    };

    let side = match body.side.to_lowercase().as_str() {
        "buy" => OrderSide::Buy,
        "sell" => OrderSide::Sell,
        _ => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "success": false,
                "message": "invalid side".to_string()
            }));
        }
    };

    if body.quantity <= 0.0 {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": "invalid quantity must be greater than zero".to_string()
        }));
    };

    let order = Order {
        id: Uuid::new_v4().to_string(),
        user_id: user_id,
        quantity: body.quantity,
        side,
        remaining_quantity: body.quantity,
        order_type: OrderType::MarketOrder,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        price: None,
    };

    let (tx, rx) = tokio::sync::oneshot::channel();

    let cmd = OrderbookCommand::AddOrder {
        order,
        response: tx,
    };

    if data.orderbook_tx.send(cmd).await.is_err() {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": "failed to add order ".to_string()
        }));
    };

    match rx.await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(_) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "success": false,
                "message": "failed to receive response".to_string()
            }));
        }
    }
}

#[derive(Deserialize)]
pub struct LimitOrderRequest {
    pub quantity: f64,
    pub price: f64,
    pub side: String,
}

#[post("/create-limit-order")]
pub async fn create_limit_order(
    data: web::Data<AppState>,
    body: Json<LimitOrderRequest>,
    req: HttpRequest,
) -> impl Responder {
    let username = match get_username_from_token(data.clone(), req) {
        Some(u) => u,
        None => {
            return HttpResponse::Unauthorized().json(serde_json::json!({
                "success": false,
                "message": "unauthorized access".to_string()
            }));
        }
    };

    let user_id = {
        let users = data.users.lock().unwrap();

        match users.get(&username) {
            Some(u) => u.id.clone(),
            None => {
                return HttpResponse::Unauthorized().json(serde_json::json!({
                    "success": false,
                    "message": "unable to find user".to_string()
                }));
            }
        }
    };

    let side = match body.side.to_lowercase().as_str() {
        "buy" => OrderSide::Buy,
        "sell" => OrderSide::Sell,
        _ => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "success": false,
                "message": "invalid side".to_string()
            }));
        }
    };

    if body.price <= 0.0 || body.quantity <= 0.0 {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "message": "price and quantity must be positive"
        }));
    }

    let order = Order {
        id: Uuid::new_v4().to_string(),
        order_type: OrderType::LimitOrder,
        price: Some(body.price),
        quantity: body.quantity,
        remaining_quantity: body.quantity,
        side: side,
        user_id: user_id,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    let cmd = OrderbookCommand::AddOrder {
        order,
        response: tx,
    };

    if data.orderbook_tx.send(cmd).await.is_err() {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "message": "unable to place order".to_string()
        }));
    }

    match rx.await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(_) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "success": false,
                "message": "unable to receive response message".to_string()
            }));
        }
    }
}

fn get_username_from_token(data: web::Data<AppState>, req: HttpRequest) -> Option<String> {
    let token_opt = req
        .headers()
        .get("Authorization")
        .and_then(|bt| bt.to_str().ok())
        .and_then(|t| t.strip_prefix("Bearer "))
        .map(|s| s.to_string());

    let token = token_opt.unwrap();

    let sessions = data.sessions.lock().unwrap();

    sessions.get(&token).cloned()
}
