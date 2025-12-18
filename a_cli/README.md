# a_cli

A minimal grep-like command-line interface (CLI) written in Rust for demonstration purposes.

## Overview

This tiny CLI searches a file for a given pattern and prints the matching lines. It demonstrates basic Rust tooling usage, including Clap for argument parsing (derive) and Indicatif for a small progress bar.

## Quick start

Prerequisites: Rust toolchain (Rustup) and Cargo.

Build the project:

```
cargo build
```

Build a release version:

```
cargo build --release
```

Run the CLI with required arguments:

```
cargo run -- --pattern "SEARCH" --path path/to/file.txt
```

Alternatively, after building you can run the binary directly from target/debug or target/release:

```
./target/debug/a_cli --pattern "SEARCH" --path path/to/file.txt
```

## Usage

The CLI accepts two required arguments:

- --pattern: The string to search for
- --path: Path to the file to search

## Example

1) Create a sample file:

```
echo -e "hello world\nfoo bar\nhello again" > sample.txt
```

2) Run the CLI to search for the word "hello":

```
cargo run -- --pattern "hello" --path sample.txt
```

Expected output: lines containing the word hello are printed and a progress indicator shows progress.

## Design notes

- This is a simple, educational example and not a feature-complete grep utility.
- For production tooling, consider robust error handling, performance improvements, and feature parity with real tools.

## Contributing

- PRs welcome. Please follow standard Rust project conventions and include tests where appropriate.

## License

MIT License

## Acknowledgments

- This project uses Clap (derive) and Indicatif for progress UI.
