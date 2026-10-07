# Pitcairn Research Hub frontend

React 19, TypeScript, Vite, React Router data router, TanStack Query and Tailwind CSS 4. Run `npm ci`, then `npm run dev`; the development server proxies `/api` and `/up` to the Rust server at `127.0.0.1:8080`. `npm run build` writes `dist/`.

## Feature folders

Put each area in `src/features/<area>/` with `api.ts` for query hooks, components and `routes.tsx` exporting route objects. Add one spread line for its routes in `src/app/routes.tsx`. DTOs must come from `src/api/generated/` via `src/api/types.ts`; regenerate them from Rust with `cd ../backend && cargo test export_bindings` or `cargo run -- export-types`. Do not maintain frontend copies of payload shapes.

The interim dashboard, project overview and neutral project tabs are foundations for the parallel backend slices. Replace individual tab route elements as those slices land. Shared controls live in `src/ui/`; upload sessions and document slots live in `src/features/upload/`.

## Checks

`npm run typecheck`, `npm run lint`, `npm test`, `npm run build`.
