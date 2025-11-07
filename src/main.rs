use std::{collections::HashMap, sync::Mutex};

use actix_web::{App, HttpRequest, HttpResponse, HttpServer, Responder, get, post, web};
use bcrypt::{DEFAULT_COST, hash, verify};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{OrderbookCommand, User};

mod orderbook;
mod types;

#[get("/hello/{name}")]
async fn greet(name: web::Path<String>) -> impl Responder {
    format!("hello {name}")
}

struct AppState {
    users: Mutex<HashMap<String, types::User>>,
    sessions: Mutex<HashMap<String, String>>,
    orderbook_tx: tokio::sync::mpsc::Sender<OrderbookCommand>,
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

#[derive(Deserialize)]
struct OnRampRequest {
    amount: f64,
}

#[derive(Serialize)]
struct OnRampResponse {
    success: bool,
    message: String,
    new_balance: f64,
}

#[post("/signup")]
async fn signup(data: web::Data<AppState>, body: web::Json<AuthRequest>) -> impl Responder {
    let username = body.username.to_string();
    let password = body.password.to_string();

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

#[get("/whoami")]
async fn whoami(data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
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

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let (tx, rx) = tokio::sync::mpsc::channel::<OrderbookCommand>(100);

    tokio::spawn(async move {
        orderbook::Orderbook::run_orderbook_engine(rx).await;
    });

    let state = web::Data::new(AppState {
        users: Mutex::new(HashMap::new()),
        sessions: Mutex::new(HashMap::new()),
        orderbook_tx: tx,
    });

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .service(greet)
            .service(signup)
            .service(whoami)
            .service(signin)
    })
    .bind(("0.0.0.0", 8000))?
    .run()
    .await
}
