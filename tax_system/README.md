# Tax System

A Rust REST API for managing products with state/year-based taxation.

## Tech Stack

- **Axum** - Web framework
- **Diesel** - ORM with SQLite backend
- **Tokio** - Async runtime

## Setup

1. Create a `.env` file:
   ```
   DATABASE_URL=tax_system.db
   ```

2. Run the server:
   ```bash
   cargo run
   ```

Server runs on `http://localhost:3000`.

## API Endpoints

| Method | Endpoint         | Description          |
|--------|------------------|----------------------|
| GET    | /products/{id}   | Get product by ID    |
| POST   | /products        | Create a new product |

### Example

```bash
curl -X POST http://localhost:3000/products \
  -H "Content-Type: application/json" \
  -d '{"product_name": "phone", "product_value": 100}'
```

## Data Models

**Product**: id, product_name, product_value

**Tax**: id, state_name, year, percent, product_id (FK)
