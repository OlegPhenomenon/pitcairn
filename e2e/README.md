# Browser acceptance tests

Run the complete Chromium suite headlessly:

```sh
cd e2e
npm ci
npx playwright install chromium
npm test
```

The run builds `../frontend/dist` if it is missing, creates a fresh temporary data directory, seeds it with `seed-demo --reset`, and serves the built frontend through the Rust backend at `127.0.0.1:18080`. Playwright waits for `/up`, runs the tests with one worker, then stops the server and removes the temporary data. On this development machine Cargo reuses `/Users/oleghasjanov/projects/pitcairn/backend/target`; elsewhere it uses Cargo's default or `CARGO_TARGET_DIR` if set.

The `webServer` launcher performs preparation before serving because Playwright starts web servers before calling `globalSetup`. Both entry points call the same idempotent preparation function. Every `npm test` invocation reseeds a new directory. The test files cover the new researcher story, access revocation, bank notification deduplication, and a 360×740 mobile viewport.

For a single test while developing, run `npx playwright test tests/full-story.spec.ts`. Failure traces are saved in `test-results/` and can be opened with `npx playwright show-trace <trace.zip>`.
