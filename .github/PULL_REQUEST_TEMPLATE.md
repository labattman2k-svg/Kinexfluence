## Summary

<!-- One sentence: what does this PR do? -->

## Standards checklist

- [ ] Tests added or updated for every changed public function
- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] UI: `npm --prefix ui run build` passes
- [ ] Extension: `npm --prefix extension run build` passes
- [ ] `CHANGELOG.md` updated under `[Unreleased]`
- [ ] No `unwrap`/`expect` outside tests
- [ ] No `any` in TypeScript without an inline justification comment
- [ ] No PII in log macros
- [ ] Public functions have doc comments

## Auditability

- [ ] Every new mutation to project/item/probe/pack writes to `event_log`
- [ ] If the schema changed, a new migration was added (existing migrations untouched)
- [ ] If the change is architectural, an ADR was added under `docs/adr/`

## Privacy

- [ ] No new network calls outside 127.0.0.1
- [ ] No new telemetry
- [ ] No user content logged above DEBUG

## Standards trade-offs

<!-- Name the standard honored and any standard deferred, with an issue link. -->

## Test evidence

<!-- Paste commands run and their output, or a link to a CI run. -->