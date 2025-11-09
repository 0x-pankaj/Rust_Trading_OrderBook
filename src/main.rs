use actix_web::{App, HttpServer, Responder, get, web};
use std::{collections::HashMap, sync::Mutex};

use crate::orders::{create_limit_order, create_market_order};
use crate::user::{me, onramp, signin, signup};
use crate::{orders::get_orderbook, types::OrderbookCommand};

mod orderbook;
mod orders;
mod types;
mod user;

#[get("/hello/{name}")]
async fn greet(name: web::Path<String>) -> impl Responder {
    format!("hello {name}")
}

struct AppState {
    users: Mutex<HashMap<String, types::User>>,
    sessions: Mutex<HashMap<String, String>>,
    orderbook_tx: tokio::sync::mpsc::Sender<OrderbookCommand>,
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
            .service(me)
            .service(signin)
            .service(get_orderbook)
            .service(onramp)
            .service(create_market_order)
            .service(create_limit_order)
    })
    .bind(("0.0.0.0", 8000))?
    .run()
    .await
}
