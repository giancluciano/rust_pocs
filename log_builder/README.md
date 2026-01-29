# Log Builder

A Logger Builder Router System is an architectural pattern for centralized logging where a service routes log events to multiple destinations (like filesystem and ELK stack) based on configurable rules.

## Core Concept

The system acts as a middleware layer that intercepts logging calls, enriches them with metadata, and intelligently routes them to appropriate destinations based on log level, source, or custom criteria.

## Prerequisites

- Rust
- Docker

## Setup

### 1. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. Start Elasticsearch

Start Docker:
```bash
docker compose up
```

### 4. Run

```bash
cargo run
```

The server starts on `http://localhost:3000`.

## Usage

Once running, the application:
- Indexes a startup message to Elasticsearch under the `system` index
- Lists all available Elasticsearch indices
- Serves HTTP requests on port 3000

Test the endpoint:
```bash
curl http://localhost:3000
```

## Running Tests

```bash
cargo test
```

## Tech Stack

- **Axum** - Web framework
- **Tokio** - Async runtime
- **Elasticsearch** - Log storage and indexing