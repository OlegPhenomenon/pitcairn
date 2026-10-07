# Contributing

## Workflow

- Branch from `main`, keep changes focused, and open a pull request (or push
  the branch for review). Commit messages: short imperative summary of what
  and why.
- `docs/architecture.md` is the binding contract: if code and the document
  would disagree, fix the code or update the document **in the same
  change**.

## Checks to run before pushing

```sh
cd backend  && cargo fmt --check && cargo clippy -- -D warnings && cargo test
cd frontend && npm run typecheck && npm run lint && npm test
cd e2e      && npm test          # Playwright acceptance suite (Chromium)
```

- Rust: `cargo fmt` and `cargo clippy` clean.
- Frontend: eslint and `tsc --noEmit` clean; DTO types are generated — edit
  the Rust structs and run `cargo run -- export-types` (or
  `cargo test export_bindings`), never hand-edit
  `frontend/src/api/generated/`.

## Data and secrets

- **No real personal data in fixtures, seeds or tests.** All demo people,
  organisations, emails (use `*.invalid`) and documents are fictional.
- **No secrets.** Keep real values in `.env` / `.env.deploy` /
  `.kamal/secrets` (all git-ignored); only `.env.example` and
  `.kamal/secrets.example` placeholders belong in the repo.
- External integrations stay mocked behind their interfaces — don't wire
  live mail/bank/payment calls into the demo.
