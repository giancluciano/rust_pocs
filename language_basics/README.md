# language_basics

A tiny Rust program illustrating variables, mutability, constants, and shadowing.

## Description

This repository contains a minimal Rust project that demonstrates:

- Variables are immutable by default
- Constants
- Shadowing to rebind variables
- Basic main function and print statements

## Installation

Prerequisites: Rust toolchain (rustc and cargo). If you don't have Rust, install via rustup: https://rust-lang.org/tools/install

## Build

cargo build

## Run

cargo run

## Code structure

- Cargo.toml: package language_basics, version 0.1.0, edition 2024
- src/main.rs: prints values and demonstrates shadowing

## Example output

The value of x is: 5
The value of x in the inner scope is: 10
The value of x is: 6

## Notes

- The code is intentionally simple to teach Rust basics.

## License

MIT License
