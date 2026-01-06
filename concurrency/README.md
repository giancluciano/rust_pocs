# Concurrency PoC (Rust)

A small Rust program showcasing basic concurrency primitives:
- spawning threads
- using channels for data transfer
- mutexes and atomic-like shared state with Arc
- a minimal async example using the trpl crate

## What this project is
This is a compact proof-of-concept (POC) intended for educational purposes. It demonstrates how to:
- create and join threads
- pass data between threads with channels
- share and mutate state safely using Mutex and Arc
- perform a simple async operation using an async runtime provided by trpl

## Code overview
- src/main.rs: The main entry point showing several concurrency patterns.
  - Simple thread creation and joining
  - Thread builder with named threads
  - Channels for sending a value across threads
  - Mutex and Arc-based shared state
  - Basic async usage with trpl::block_on
  - Async function page_title that fetches a page title
- Cargo.toml: Project metadata and dependencies
  - trpl = "0.3.0"

## How it works (high level)
- Spawns multiple threads to print messages and to increment a shared counter guarded by a Mutex inside an Arc.
- Uses a channel to send a string from a worker thread back to the main thread.
- Demonstrates basic async integration by awaiting a HTTP GET and parsing the HTML title (via trpl).

## Build
- Ensure you have Rust 1.60+ (edition 2024) installed.
- Build the project:

```
cargo build
```

## Run
- Run the binary:

```
cargo run
```

You should see several prints from different threads, followed by the final counter value and the fetched page title for google.com (depending on network access).

## Code map (quick)
- src/main.rs
- Cargo.toml

## Dependencies
- trpl = "0.3.0"

## Known caveats and future improvements
- The async example uses a simple runtime and may not be suitable for production workloads.
- Error handling in the async part is minimal for brevity.
- Could enhance with proper error handling, tests, and more robust concurrency patterns.

## License
- License: TBD

## Contact
- This repository is a teaching/example POCR. For feedback, open an issue or PR in the repository.
