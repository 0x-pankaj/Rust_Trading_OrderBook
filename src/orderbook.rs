use std::collections::{BTreeMap, VecDeque};

use uuid::Uuid;

use crate::types::{
    Order, OrderResponse, OrderSide, OrderType, OrderbookCommand, OrderbookSnapshot, Trade,
};

pub struct Orderbook {
    bids: BTreeMap<u64, VecDeque<Order>>,
    asks: BTreeMap<u64, VecDeque<Order>>,
}

impl Orderbook {
    pub fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
        }
    }

    fn price_to_key(price: f64) -> u64 {
        (price * 100000.0) as u64
    }

    fn key_to_price(cent: u64) -> f64 {
        cent as f64 / 100000.0
    }

    pub fn add_order(&mut self, mut order: Order) -> OrderResponse {
        let mut trades = Vec::new();
        let original_quantity = order.quantity;

        match order.order_type {
            OrderType::MarketOrder => {
                trades = self.match_market_order(&mut order);

                if trades.is_empty() {
                    return OrderResponse::Error {
                        message: "No matching orders available".to_string(),
                    };
                }

                let filled_quantity = original_quantity - order.remaining_quantity;

                if order.remaining_quantity > 0.0 {
                    OrderResponse::PartiallyFilled {
                        order_id: order.id.clone(),
                        filled_quantity,
                        remaining_quantity: order.remaining_quantity,
                        trades,
                    }
                } else {
                    OrderResponse::Filled {
                        order_id: order.id.clone(),
                        filled_quantity: original_quantity,
                        trades,
                    }
                }
            }
            OrderType::LimitOrder => {
                if order.price.is_none() {
                    return OrderResponse::Error {
                        message: "limit order must have price".to_string(),
                    };
                }

                trades = self.match_limit_order(&mut order);

                if order.remaining_quantity > 0.0 {
                    self.add_to_book(order.clone());

                    if trades.is_empty() {
                        OrderResponse::Placed {
                            order_id: order.id.clone(),
                        }
                    } else {
                        OrderResponse::PartiallyFilled {
                            order_id: order.id.clone(),
                            filled_quantity: original_quantity - order.quantity,
                            remaining_quantity: order.remaining_quantity,
                            trades,
                        }
                    }
                } else {
                    OrderResponse::Filled {
                        order_id: order.id.clone(),
                        filled_quantity: order.quantity,
                        trades,
                    }
                }
            }
        }
    }

    pub fn match_market_order(&mut self, order: &mut Order) -> Vec<Trade> {
        let mut trades = Vec::new();
        let book = match order.side {
            OrderSide::Buy => &mut self.asks,
            OrderSide::Sell => &mut self.bids,
        };

        let keys: Vec<u64> = match order.side {
            OrderSide::Buy => book.keys().copied().collect(),
            OrderSide::Sell => book.keys().copied().rev().collect(),
        };

        for price_key in keys {
            if (order.remaining_quantity <= 0.0) {
                break;
            }

            if let Some(order_at_price) = book.get_mut(&price_key) {
                while let Some(mut matching_order) = order_at_price.pop_front() {
                    let trade_quantity = order
                        .remaining_quantity
                        .min(matching_order.remaining_quantity);
                    let trade_price = matching_order.price.unwrap();

                    let trade = Trade {
                        id: Uuid::new_v4().to_string(),
                        buy_order_id: match order.side {
                            OrderSide::Buy => order.id.clone(),
                            OrderSide::Sell => matching_order.id.clone(),
                        },
                        sell_order_id: match order.side {
                            OrderSide::Buy => matching_order.id.clone(),
                            OrderSide::Sell => order.id.clone(),
                        },
                        price: trade_price,
                        quantity: trade_quantity,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    };

                    trades.push(trade);

                    order.remaining_quantity -= trade_quantity;
                    matching_order.remaining_quantity -= trade_quantity;

                    if matching_order.remaining_quantity > 0.0 {
                        order_at_price.push_front(matching_order);
                    }

                    if order.remaining_quantity <= 0.0 {
                        break;
                    }
                }

                if order_at_price.is_empty() {
                    book.remove(&price_key);
                }
            }
        }
        trades
    }

    pub fn match_limit_order(&mut self, order: &mut Order) -> Vec<Trade> {
        let mut trades = Vec::new();
        let order_price = order.price.unwrap();

        let book = match order.side {
            OrderSide::Buy => &mut self.asks,
            OrderSide::Sell => &mut self.bids,
        };

        let keys: Vec<u64> = match order.side {
            OrderSide::Buy => book.keys().copied().collect(),
            OrderSide::Sell => book.keys().copied().rev().collect(),
        };

        for price_key in keys {
            let matching_price = Self::key_to_price(price_key);

            let should_match = match order.side {
                OrderSide::Buy => order_price >= matching_price,
                OrderSide::Sell => order_price <= matching_price,
            };

            if !should_match {
                break;
            }

            if order.remaining_quantity <= 0.0 {
                break;
            }

            if let Some(order_at_price) = book.get_mut(&price_key) {
                while let Some(mut matching_order) = order_at_price.pop_front() {
                    let trading_quantity = order
                        .remaining_quantity
                        .min(matching_order.remaining_quantity);
                    let trade_price = matching_order.price.unwrap();

                    let trade = Trade {
                        id: Uuid::new_v4().to_string(),
                        buy_order_id: match order.side {
                            OrderSide::Buy => order.id.clone(),
                            OrderSide::Sell => matching_order.id.clone(),
                        },
                        sell_order_id: match order.side {
                            OrderSide::Buy => matching_order.id.clone(),
                            OrderSide::Sell => order.id.clone(),
                        },
                        price: trade_price,
                        quantity: trading_quantity,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    };

                    trades.push(trade);

                    order.remaining_quantity -= trading_quantity;
                    matching_order.remaining_quantity -= trading_quantity;

                    if (matching_order.remaining_quantity > 0.0) {
                        order_at_price.push_front(matching_order);
                    }

                    if order.remaining_quantity <= 0.0 {
                        break;
                    }
                }
                if order_at_price.is_empty() {
                    book.remove(&price_key);
                }
            }
        }
        trades
    }

    fn add_to_book(&mut self, order: Order) {
        let price = order.price.unwrap();
        let price_key = Self::price_to_key(price);

        let book = match order.side {
            OrderSide::Buy => &mut self.bids,
            OrderSide::Sell => &mut self.asks,
        };

        book.entry(price_key)
            .or_insert_with(VecDeque::new)
            .push_back(order);
    }

    pub fn get_snapshot(&mut self) -> OrderbookSnapshot {
        let mut bids = Vec::new();
        for (price_key, orders) in self.bids.iter().rev() {
            let total_quantity = orders.iter().map(|o| o.remaining_quantity).sum();
            bids.push((Self::key_to_price(*price_key), total_quantity));
        }

        let mut asks = Vec::new();
        for (price_key, orders) in self.asks.iter() {
            let total_quantity = orders.iter().map(|o| o.remaining_quantity).sum();
            asks.push((Self::key_to_price(*price_key), total_quantity));
        }

        OrderbookSnapshot { bids, asks }
    }

    pub async fn run_orderbook_engine(mut rx: tokio::sync::mpsc::Receiver<OrderbookCommand>) {
        let mut orderbook = Orderbook::new();

        while let Some(command) = rx.recv().await {
            match command {
                OrderbookCommand::AddOrder { order, response } => {
                    let result = orderbook.add_order(order);
                    let _ = response.send(result);
                }
                OrderbookCommand::GetSnapshot { response } => {
                    let snapshot = orderbook.get_snapshot();
                    let _ = response.send(snapshot);
                }
            }
        }
    }
}
