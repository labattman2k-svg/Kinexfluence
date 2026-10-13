# ADR 0004 — Extension ships via official stores

Status: accepted
Date: 2026-10-13

## Context
Store review adds latency but reaches non-technical users and auto-updates.

## Decision
Ship to Chrome Web Store and Firefox Add-ons. Maintain a signed dev build
for early testers and adapter contributors.

## Consequences
- Two distribution channels to maintain.
- Store review becomes part of the release cadence.
