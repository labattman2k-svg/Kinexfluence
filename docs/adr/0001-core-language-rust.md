# ADR 0001 — Core language is Rust

Status: accepted
Date: 2026-10-13

## Context
The core performs many small hot operations (scoring, embeddings lookup,
graph traversal) and must ship as a single cross-platform binary. Tauri
already uses Rust.

## Decision
Implement the core in Rust.

## Consequences
- Single language across core and shell.
- One binary per platform, no Python or Node runtime required.
- Rust's ecosystem covers SQLite (sqlx), ONNX (ort), and HTTP (axum).
- Slightly higher contributor bar than Go or TypeScript.
