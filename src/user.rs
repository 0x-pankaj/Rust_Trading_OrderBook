use actix_web::{
    HttpRequest, HttpResponse, Responder, get, post,
    web::{self, Json},
};
use bcrypt::{DEFAULT_COST, hash, verify};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::types::User;

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
            let mut users = data.users.lock().unwrap();
            if let Some(user) = users.get_mut(username) {
                if body.amount <= 0.0 {
                    return HttpResponse::BadRequest().json(OnRampResponse {
                        success: false,
                        message: "amount must be greate than zero ".to_string(),
                        new_balance: 0.0,
                    });
                }

                user.balance += body.amount;
                HttpResponse::Ok().json(OnRampResponse {
                    success: true,
                    message: "successfully added balance ".to_string(),
                    new_balance: user.balance,
                })
            } else {
                HttpResponse::Unauthorized().json(OnRampResponse {
                    success: false,
                    message: "unauthorized".to_string(),
                    new_balance: 0.0,
                })
            }
        }
        None => HttpResponse::Unauthorized().json(OnRampResponse {
            success: false,
            message: "unautorized ".to_string(),
            new_balance: 0.0,
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

    println!("token: {}", token);

    let sessions = data.sessions.lock().unwrap();

    match sessions.get(&token) {
        Some(user) => HttpResponse::Ok().json(serde_json::json!({"username": user})),
        None => return HttpResponse::Unauthorized().body("invalid token"),
    }
}
