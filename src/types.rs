use std::collections::HashMap;

#[derive(Clone)]
pub struct User {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub balance: f64,
    pub assets: HashMap<String, f64>,
}

impl User {
    pub fn new(id: String, username: String, password: String) -> Self {
        Self {
            id,
            username,
            password_hash: password,
            balance: 0.0,
            assets: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone)]
pub enum OrderType {
    LimitOrder,
    MarketOrder,
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: String,
    pub user_id: String,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub price: Option<f64>,
    pub quantity: f64,
    pub remaining_quantity: f64,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct Trade {
    pub id: String,
    pub buy_order_id: String,
    pub sell_order_id: String,
    pub price: f64,
    pub quantity: f64,
    pub timestamp: u64,
}

pub struct OrderbookSnapshot {
    pub bids: Vec<(f64, f64)>,
    pub asks: Vec<(f64, f64)>,
}

pub enum OrderResponse {
    Placed {
        order_id: String,
    },
    PartiallyFilled {
        order_id: String,
        filled_quantity: f64,
        remaining_quantity: f64,
        trades: Vec<Trade>,
    },
    Filled {
        order_id: String,
        filled_quantity: f64,
        trades: Vec<Trade>,
    },
    Cancelled {
        order_id: String,
    },
    Error {
        message: String,
    },
}

pub enum OrderbookCommand {
    AddOrder {
        order: Order,
        response: tokio::sync::oneshot::Sender<OrderResponse>,
    },
    GetSnapshot {
        response: tokio::sync::oneshot::Sender<OrderbookSnapshot>,
    },
}
