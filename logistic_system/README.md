# Logistic System

A REST API for calculating dynamic freight prices based on volume, size, and transportation type. Built with Rust, Actix-web, and SQLite.

## How It Works

The system looks up a **base price** for the given transport type from the database, then applies the formula:

```
price = base_price + (volume * size)
```

Supported transport types: `boat`, `truck`, `rail`.

## Tech Stack

- **Actix-web 4** - HTTP server
- **SQLx** - async SQLite with compile-time checked queries
- **Tokio** - async runtime
- **Serde** - JSON serialization

## Getting Started

### Prerequisites

- Rust (2021 edition)
- SQLx CLI (optional, for manual migrations)

### Run

```bash
cargo run
```

The server starts at `http://localhost:8080`. Database migrations run automatically on startup, creating the `freight_price` table and seeding initial data.

### API

**POST** `/calculate_price`

Request:
```json
{
  "volume": 2.0,
  "size": 3.0,
  "type_transport": "truck"
}
```

Response (`200 OK`):
```json
{
  "price": 106.0
}
```


### Example

```bash
curl -X POST http://localhost:8080/calculate_price \
    -H "Content-Type: application/json" \
    -d '{"volume": 2.0, "size": 3.0, "type_transport": "truck"}'
```
