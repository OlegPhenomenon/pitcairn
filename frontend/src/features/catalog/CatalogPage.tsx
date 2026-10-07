import { Link } from 'react-router';
import { Anchor } from 'lucide-react';

import { EmptyState } from '../../ui';
import { useApiQuery } from '../../api/client';
import type { ListResponse, PublicProjectDto } from '../../api/types';

/**
 * /catalog — public results catalog. The public API arrives with a later
 * slice; until then (or when the catalog is disabled) we show a calm empty
 * state instead of an error.
 */
export function CatalogPage() {
  const list = useApiQuery<ListResponse<PublicProjectDto>>(
    ['public', 'projects'],
    '/public/projects',
    { retry: false },
  );
  const items = (list.data?.items ?? []).filter((p) => p.reference !== null);

  return (
    <div className="flex min-h-dvh flex-col bg-sand-50">
      <header className="bg-navy-900 text-white">
        <div className="mx-auto flex w-full max-w-6xl items-center gap-3 px-4 py-3">
          <Link to="/" className="inline-flex items-center gap-2 font-semibold">
            <Anchor className="size-5 text-teal-300" aria-hidden />
            Pitcairn Research Hub
          </Link>
          <nav className="ml-auto" aria-label="Public">
            <Link
              to="/login"
              className="rounded-md bg-teal-700 px-3 py-2 text-sm font-medium text-white hover:bg-teal-600"
            >
              Log in
            </Link>
          </nav>
        </div>
      </header>
      <main id="main" className="mx-auto w-full max-w-6xl flex-1 px-4 py-8">
        <h1 className="text-2xl font-bold text-navy-900">Open catalog</h1>
        <p className="mt-1 text-sm text-slate-600">
          Results published by research projects on the Pitcairn Islands.
        </p>
        <div className="mt-6">
          {list.isPending ? (
            <p className="text-sm text-slate-500">Loading…</p>
          ) : list.isError || items.length === 0 ? (
            <EmptyState
              title="Nothing published yet"
              body="When research projects deliver their results and the coordinator publishes them, they will appear here — no account needed."
            />
          ) : (
            <ul className="grid gap-3">
              {items.map((p) => (
                <li
                  key={p.reference}
                  className="rounded-lg border border-slate-200 bg-white p-4 shadow-sm"
                >
                  <Link
                    to={`/catalog/${p.reference}`}
                    className="font-semibold text-teal-800 hover:underline"
                  >
                    {p.title}
                  </Link>
                  <p className="text-sm text-slate-500">
                    {p.reference} · {p.organisation}
                  </p>
                </li>
              ))}
            </ul>
          )}
        </div>
      </main>
      <footer className="border-t border-slate-200 py-4 text-center text-xs text-slate-500">
        Open-source demo · fictional data
      </footer>
    </div>
  );
}
