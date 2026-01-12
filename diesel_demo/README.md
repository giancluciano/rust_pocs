# Diesel Demo (SQLite) - README

A minimal Rust + Diesel ORM example using SQLite with migrations.

## Prerequisites
- Rust toolchain (Cargo)
- SQLite development libraries (e.g., sqlite3 and libsqlite3-dev on Linux)
- Diesel CLI with SQLite support

## Install dependencies
- Ensure Rust and Cargo are installed. Then install Diesel CLI:

```
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/diesel-rs/diesel/releases/latest/download/diesel_cli-installer.sh | sh
```

## Database setup
- Create a database file inside the project, e.g. `diesel_demo/database.db`.
- Set the DATABASE_URL environment variable to point to this file:

Linux/macOS:
```
echo DATABASE_URL=/path/to/your/sqlite/database.db > .env
```


## Migrate and run
1) Run migrations to create the schema defined in migrations/up.sql
```
diesel migration run
```

2) Build and run the Rust project
```
cargo build
 cargo run
```

## Verification
- The migrations create a `posts` table with columns: `id`, `title`, `body`, `published`.
- You can inspect the SQLite database with the sqlite3 CLI:
```
sqlite3 diesel_demo/db.sqlite ".schema posts"
```

## Project structure (highlights)
- Cargo.toml: project dependencies
- src/main.rs: entry point (prints a greeting)
- src/schema.rs: Diesel table schema (generated)
- migrations/2026-01-13-200441-0000_create_posts/up.sql
- migrations/2026-01-13-200441-0000_create_posts/down.sql

## Notes
- The app currently prints "Hello, world!" on run. This is a minimal PoC to demonstrate Diesel integration with SQLite and migrations.
- The migrations directory contains a creation script for `posts`.

## Cleanup (optional)
- Remove or reset the `diesel_demo/db.sqlite` file to start fresh.
