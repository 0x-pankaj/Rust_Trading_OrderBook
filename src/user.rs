use actix_web::{
    HttpRequest, HttpResponse, Responder, get, post,
    web::{self, Json},
};
use bcrypt::{DEFAULT_COST, hash, verify};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::types::User;
use crate::{AppState, types::OrderbookCommand};

#[derive(Deserialize)]
pub struct OnRampRequest {
    pub amount: f64,
}

#[derive(Serialize)]
pub struct OnRampResponse {
    pub success: bool,
    pub message: String,
    pub new_balance: f64,
}

#[post("/onramp")]
pub async fn onramp(
    data: web::Data<AppState>,
    body: Json<OnRampRequest>,
    req: HttpRequest,
) -> impl Responder {
    let token_opt = req
        .headers()
        .get("Authorization")
        .and_then(|bt| bt.to_str().ok())
        .and_then(|t| t.strip_prefix("Bearer "))
        .map(|s| s.to_string());

    if token_opt.is_none() {
        return HttpResponse::Unauthorized().json(OnRampResponse {
            success: false,
            message: "missing authorization token".to_string(),
            new_balance: 0.0,
        });
    }

    let token = token_opt.unwrap();

    let sessions = data.sessions.lock().unwrap();

    match sessions.get(&token) {
        Some(username) => {
            let user_id = {
                let users = data.users.lock().unwrap();
                match users.get(username) {
                    Some(user) => user.id.clone(),
                    None => {
                        return HttpResponse::Unauthorized().json(OnRampResponse {
                            message: "usernot found".to_string(),
                            new_balance: 0.0,
                            success: false,
                        });
                    }
                }
            };
            if body.amount < 0.0 {
                return HttpResponse::BadRequest().json(OnRampResponse {
                    message: "balance must be greater than zero".to_string(),
                    new_balance: 0.0,
                    success: false,
                });
            }

            // sending to orderbook
            let (tx, rx) = oneshot::channel();
            let cmd = OrderbookCommand::OnRamp {
                user_id,
                amount: body.amount,
                response: tx,
            };

            if data.orderbook_tx.send(cmd).await.is_err() {
                return HttpResponse::InternalServerError().json(OnRampResponse {
                    message: "failed to send to orderbook".to_string(),
                    new_balance: 0.0,
                    success: false,
                });
            }

            match rx.await {
                Ok(res) => HttpResponse::Ok().json(OnRampResponse {
                    message: res.message,
                    new_balance: res.new_balance,
                    success: res.success,
                }),
                Err(_) => HttpResponse::InternalServerError().json(OnRampResponse {
                    message: "Failed to receive response".to_string(),
                    new_balance: 0.0,
                    success: false,
                }),
            }
        }
        None => HttpResponse::Unauthorized().json(OnRampResponse {
            message: "unauthorized".to_string(),
            new_balance: 0.0,
            success: false,
        }),
    }
}

#[derive(Serialize)]
struct AuthResponse {
    success: bool,
    message: String,
    token: Option<String>,
}
#[derive(Deserialize)]
struct AuthRequest {
    username: String,
    password: String,
}

#[post("/signup")]
async fn signup(data: web::Data<AppState>, body: web::Json<AuthRequest>) -> impl Responder {
    let username = body.username.to_string();
    let password = body.password.to_string();
    println!("called");
    if username.is_empty() || password.is_empty() {
        return HttpResponse::BadRequest().json(AuthResponse {
            success: false,
            message: "user and password cannot be empty".into(),
            token: None,
        });
    }

    let mut users = data.users.lock().unwrap();

    if users.contains_key(&username) {
        return HttpResponse::Conflict().json(AuthResponse {
            success: false,
            message: "user already exists".into(),
            token: None,
        });
    }

    let password_hash = match hash(&password, DEFAULT_COST) {
        Ok(h) => h,
        Err(_) => {
            return HttpResponse::InternalServerError().json(AuthResponse {
                success: false,
                message: format!("failed to hash the password"),
                token: None,
            });
        }
    };

    let id = Uuid::new_v4().to_string();
    let user = User::new(id, username.clone(), password_hash);

    users.insert(username.clone(), user);

    HttpResponse::Ok().json(AuthResponse {
        success: true,
        message: "successfully created user".into(),
        token: None,
    })
}

#[post("/signin")]
async fn signin(data: web::Data<AppState>, body: web::Json<AuthRequest>) -> impl Responder {
    let username = body.username.to_string();
    let password = body.password.to_string();

    let user = {
        let users = data.users.lock().unwrap();
        match users.get(&username) {
            Some(u) => u.clone(),
            None => {
                return HttpResponse::Unauthorized().json(AuthResponse {
                    success: false,
                    message: "not registerd user".into(),
                    token: None,
                });
            }
        }
    };

    //verify

    match verify(password, &user.password_hash) {
        Ok(true) => {
            let token = Uuid::new_v4().to_string();
            let mut sessions = data.sessions.lock().unwrap();
            sessions.insert(token.clone(), user.username.clone());

            HttpResponse::Ok().json(AuthResponse {
                success: true,
                message: "login in successfully".into(),
                token: Some(token),
            })
        }
        _ => {
            return HttpResponse::Unauthorized().json(AuthResponse {
                success: false,
                message: "wrong credentials".into(),
                token: None,
            });
        }
    }
}

#[get("/me")]
async fn me(data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    let token_opt = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|t| t.strip_prefix("Bearer "))
        .map(|s| s.to_string());

    if token_opt.is_none() {
        return HttpResponse::Unauthorized().json(AuthResponse {
            success: false,
            message: "missing authorization token".into(),
            token: None,
        });
    }

    let token = token_opt.unwrap();
    let sessions = data.sessions.lock().unwrap();

    match sessions.get(&token) {
        Some(username) => {
            let user_id = {
                let users = data.users.lock().unwrap();
                match users.get(username) {
                    Some(u) => u.id.clone(),
                    None => {
                        return HttpResponse::Unauthorized().json(serde_json::json!({
                            "success": false,
                            "message": "user not found".to_string()
                        }));
                    }
                }
            };

            let (tx, rx) = oneshot::channel();
            let cmd = OrderbookCommand::GetUserBalance {
                user_id,
                response: tx,
            };

            if data.orderbook_tx.send(cmd).await.is_err() {
                return HttpResponse::InternalServerError().json(serde_json::json!({
                    "success": false,
                    "message": "failed to get user balance".to_string()
                }));
            }

            match rx.await {
                Ok(balance_info) => HttpResponse::Ok().json(serde_json::json!({
                    "username": username.to_string(),
                    "balance": balance_info.collateral_balance,
                    "locked_balance": balance_info.locked_balance,
                    "assets": balance_info.assets,
                    "pending_orders": balance_info.pending_order
                })),
                Err(_) => {
                    return HttpResponse::InternalServerError().json(serde_json::json!({
                        "message": "failed to receive message".to_string(),
                        "success": false
                    }));
                }
            }
        }
        None => {
            return HttpResponse::Unauthorized().json(serde_json::json!({
                "success": false,
                "message": "invalid token".to_string()
            }));
        }
    }
}
