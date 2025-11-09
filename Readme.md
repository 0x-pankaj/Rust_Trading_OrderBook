# High-Performance Orderbook Exchange System

A multi-threaded orderbook matching engine built with Rust, Actix-web, and Tokio for handling cryptocurrency trading operations with concurrent order processing.

## 🏗️ Architecture Overview

This system implements a **single-threaded orderbook engine** running in a dedicated Tokio task, communicating with multiple HTTP handler threads through **message passing** using channels. This design ensures **thread-safety without locks** on the orderbook itself.
```
┌─────────────────────────────────────────────────────────────┐
│                     Main Application                        │
│                                                             │
│  ┌────────────┐      ┌──────────────┐     ┌──────────────┐  │
│  │ HTTP Thread│      │  HTTP Thread │     │  HTTP Thread │  │
│  │   (Actix)  │      │    (Actix)   │     │    (Actix)   │  │
│  └─────┬──────┘      └──────┬───────┘     └──────┬───────┘  │
│        │                    │                    │          │
│        │    mpsc::Sender    │       mpsc::Sender │          │
│        └────────────────────┼────────────────────┘          │
│                             ▼                               │
│                  ┌──────────────────────┐                   │
│                  │  mpsc::Receiver (rx) │                   │
│                  └──────────┬───────────┘                   │
│                             │                               │
│                ┌────────────▼───────────────┐               │
│                │  Orderbook Engine Thread   │               │
│                │  (Single Tokio Task)       │               │
│                │                            │               │
│                │  - BTreeMap (Bids/Asks)    │               │
│                │  - HashMap (User Balances) │               │
│                │  - Order Matching Logic    │               │
│                │  - NO MUTEX NEEDED!        │               │
│                └────────────┬───────────────┘               │
│                             │                               │
│                    oneshot::Sender                          │
│                             │                               │
│                    ┌────────▼────────┐                      │
│                    │ Response Back   │                      │
│                    │ to HTTP Handler │                      │
│                    └─────────────────┘                      │
└─────────────────────────────────────────────────────────────┘
```

## 🔄 Threading Model & Communication

### Why This Architecture?

1. **Single-Threaded Orderbook**: The orderbook runs in ONE dedicated thread/task
   - No race conditions
   - No locks/mutexes needed for orderbook operations
   - Guaranteed sequential processing of orders
   - Deterministic order matching

2. **Multiple HTTP Handler Threads**: Actix-web spawns multiple worker threads
   - Each handles incoming HTTP requests concurrently
   - They don't directly access the orderbook
   - They communicate via message passing

### Channel Communication Pattern

#### MPSC (Multi-Producer, Single-Consumer) Channel
```rust
// In main.rs
let (tx, rx) = mpsc::channel::<OrderbookCommand>(100);
//   ^    ^                                        ^
//   |    |                                        |
//   |    |                                   Buffer size
//   |    |
//   |    Single consumer (orderbook thread)
//   |
//   Multiple producers (HTTP handlers)
```

**Flow:**
1. Multiple HTTP handlers (producers) send commands to orderbook
2. Single orderbook engine (consumer) receives and processes sequentially
3. Buffer size 100 means up to 100 commands can be queued

#### Oneshot Channel (Request-Response Pattern)
```rust
// In API handler
let (tx, rx) = oneshot::channel();
//   ^    ^
//   |    |
//   |    Receiver - waits for response
//   |
//   Sender - goes with command to orderbook

let cmd = OrderbookCommand::AddOrder {
    order: my_order,
    response: tx,  // Orderbook will send response back through this
};

// Send command via MPSC
data.orderbook_tx.send(cmd).await?;

// Wait for response via oneshot
let result = rx.await?;
```

**Why Oneshot?**
- Each request needs exactly ONE response
- Automatically cleaned up after single use
- Type-safe way to match requests with responses

### Complete Flow Example: Placing a Limit Order
```
[Client] ──HTTP POST /create-limit-order──▶ [Actix HTTP Handler Thread #3]
                                                    │
                                                    │ 1. Extract user auth
                                                    │ 2. Validate request
                                                    │ 3. Create Order struct
                                                    ▼
                                            ┌───────────────────┐
                                            │ Create channels:  │
                                            │ (tx, rx) oneshot │
                                            └────────┬──────────┘
                                                    │
                                    4. Create Command
                                    OrderbookCommand::AddOrder {
                                        order: Order {...},
                                        response: tx  ◄─── Send this
                                    }                │
                                                    │
                                    5. Send via MPSC│
                                    mpsc_tx.send(cmd)│
                                                    │
                        ┌───────────────────────────┼───────────────┐
                        │    CROSSES THREAD BOUNDARY│               │
                        └───────────────────────────┼───────────────┘
                                                    ▼
                                        ┌──────────────────────┐
                                        │ Orderbook Engine     │
                                        │ (Dedicated Thread)   │
                                        │                      │
                                        │ while let Some(cmd)  │
                                        │   = rx.recv().await  │
                                        └──────────┬───────────┘
                                                   │
                                    6. Process order        │
                                    - Check balance         │
                                    - Lock funds           │
                                    - Match order          │
                                    - Update orderbook     │
                                                   │
                                    7. Create response      │
                                    OrderResponse::Filled { │
                                        order_id,           │
                                        trades,             │
                                    }                       │
                                                   │
                                    8. Send back via oneshot│
                                    tx.send(response) ──────┘
                                                   │
                        ┌──────────────────────────┼───────────────┐
                        │    CROSSES THREAD BOUNDARY│               │
                        └──────────────────────────┼───────────────┘
                                                   ▼
                                    9. Receive response
                                    let result = rx.await?
                                                   │
                                    10. Return HTTP response
                                    HttpResponse::Ok().json(result)
                                                   │
[Client] ◀──────HTTP 200 OK with order details────┘
```

## 📁 Project Structure
```
src/
├── main.rs           # Application entry, channel setup, server config
├── types.rs          # Data structures (Order, Trade, Commands)
├── orderbook.rs      # Core matching engine (single-threaded)
├── order.rs          # Order API endpoints (create, cancel)
├── user.rs           # User auth APIs (signup, signin, onramp)
└── engine.rs         # (if any additional engine logic)
```

## 🔐 User Balance Management

All balance and asset tracking happens **inside the orderbook thread** to maintain consistency:
```rust
struct UserStore {
    collateral_balance: f64,      // Available balance
    locked_balance: f64,           // Locked in pending orders
    pending_orders: VecDeque<Order>, // Active limit orders
    assets: HashMap<String, f64>,  // Held assets (e.g., BTC)
}
```

### Balance Flow:

**Limit Buy Order:**
```
1. Check: collateral_balance >= price * quantity
2. Lock:  collateral_balance -= required
          locked_balance += required
3. Match: When filled:
          locked_balance -= trade_value
          assets["BTC"] += quantity
```

**Limit Sell Order:**
```
1. Check: assets["BTC"] >= quantity
2. Lock:  assets["BTC"] -= quantity (immediately)
3. Match: When filled:
          collateral_balance += trade_value
```

**Market Order:**
```
- No locking (executes immediately)
- Balance/assets checked and deducted during matching
- Partial fills return unused balance/assets
```

## 🚀 API Endpoints

### Authentication
- `POST /signup` - Create new user account
- `POST /signin` - Login and get auth token
- `GET /me` - Get user info (balance, assets, pending orders)

### Funding
- `POST /onramp` - Add collateral balance
```json
  {
    "amount": 10000.0
  }
```

### Trading
- `POST /create-limit-order` - Place limit order
```json
  {
    "side": "buy",
    "price": 50000.0,
    "quantity": 0.5
  }
```

- `POST /create-market-order` - Place market order
```json
  {
    "side": "sell",
    "quantity": 0.25
  }
```

- `POST /cancel-order` - Cancel pending order
```json
  {
    "order_id": "uuid-here"
  }
```

- `GET /get-orderbook` - View current orderbook snapshot

## 🔒 Thread Safety Guarantees

| Component | Thread Safety Mechanism |
|-----------|-------------------------|
| Orderbook Engine | Single-threaded, sequential processing |
| User Authentication | `Mutex<HashMap>` (rarely contested) |
| Order Commands | Message passing (no shared state) |
| Order Responses | Oneshot channels (one-time use) |

## ⚡ Performance Characteristics

- **Order Processing**: Sequential but extremely fast (no lock contention)
- **Concurrent Requests**: Actix handles multiple HTTP requests in parallel
- **Bottleneck**: Single orderbook thread (intentional for consistency)
- **Scalability**: Can handle thousands of orders/sec on single thread



# Server starts on http://0.0.0.0:8000
```

## 📊 Order Matching Logic

### Price-Time Priority
1. Orders sorted by price (best price first)
2. Orders at same price sorted by time (FIFO)
3. BTreeMap ensures O(log n) price level access
4. VecDeque ensures O(1) FIFO order access

### Matching Algorithm
```rust
// Buy order matches with asks (sellers)
// Sell order matches with bids (buyers)

for each price level (best to worst):
    while order has remaining quantity:
        match with first order at this level
        execute trade
        update both orders
        settle balances immediately
```

## 🧪 Testing Flow

1. **Create Users**:
```bash
   curl -X POST http://localhost:8000/signup \
     -H "Content-Type: application/json" \
     -d '{"username":"alice","password":"pass123"}'
```

2. **Add Balance**:
```bash
   curl -X POST http://localhost:8000/onramp \
     -H "Authorization: Bearer YOUR_TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"amount":100000}'
```

3. **Place Orders**:
```bash
   # Alice places sell order
   curl -X POST http://localhost:8000/create-limit-order \
     -H "Authorization: Bearer ALICE_TOKEN" \
     -d '{"side":"sell","price":50000,"quantity":1}'

   # Bob places buy order (matches!)
   curl -X POST http://localhost:8000/create-limit-order \
     -H "Authorization: Bearer BOB_TOKEN" \
     -d '{"side":"buy","price":50000,"quantity":1}'
```

4. **Check Balance**:
```bash
   curl http://localhost:8000/me \
     -H "Authorization: Bearer ALICE_TOKEN"
```

## 🔍 Key Insights

### Why Not Use Mutex for Orderbook?
Using `Arc<Mutex<Orderbook>>` would:
- ❌ Create lock contention between threads
- ❌ Reduce throughput under high load
- ❌ Risk deadlocks with complex operations
- ❌ Make reasoning about order harder

Message passing ensures:
- ✅ Sequential, deterministic order processing
- ✅ No locks on hot path
- ✅ Clear ownership (single thread owns orderbook)
- ✅ Easy to reason about state changes

### Why MPSC + Oneshot?
- **MPSC**: Multiple HTTP handlers → One orderbook
- **Oneshot**: Each request gets exactly one response back
- **Separation**: HTTP layer and matching engine are decoupled

## 📝 Future Enhancements


- [ ] Partial fill handling inside same price level
- [ ] Multiple trading pairs (not just BTC)
- [ ] Websocket for real-time orderbook updates
- [ ] Persistent storage (database)
- [ ] Circuit breakers for overload protection
- [ ] Metrics and monitoring
- [ ] Seperation of Engine from Http Server Connect Via Redis Stream.


#
### Feel free to use and modify!


---

**Built with ❤️ using Rust, Actix-web, and Tokio**
