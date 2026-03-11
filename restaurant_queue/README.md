# Restaurant Queue

A Rust proof-of-concept demonstrating a FIFO order queue for a restaurant kitchen.

## What it does

- Accepts orders from customers, each with a dish and preparation time
- Calculates ETA for each new order based on cumulative prep time in the queue
- Serves orders in FIFO order
- Displays the current queue status with per-position ETAs

## Run

```bash
cargo run
```

## Key types

| Type | Description |
|------|-------------|
| `Dish` | Name + preparation time |
| `Order` | ID, customer name, and dish |
| `RestaurantQueue` | `VecDeque`-backed queue with ETA tracking |
