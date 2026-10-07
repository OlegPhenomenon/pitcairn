import type { ReactNode } from 'react';
import { Link } from 'react-router';
import { Anchor } from 'lucide-react';

/** Centered card on sand background for login/register/mfa pages. */
export function AuthShell({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="flex min-h-dvh flex-col bg-sand-100">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2 focus:z-50 focus:rounded focus:bg-white focus:px-3 focus:py-2"
      >
        Skip to content
      </a>
      <header className="bg-navy-900 text-white">
        <div className="mx-auto flex max-w-5xl items-center gap-2 px-4 py-3">
          <Link to="/" className="inline-flex items-center gap-2 font-semibold">
            <Anchor className="size-5 text-teal-300" aria-hidden />
            Pitcairn Research Hub
          </Link>
        </div>
      </header>
      <main id="main" className="flex flex-1 items-start justify-center px-4 py-10">
        <div className="w-full max-w-md rounded-lg border border-slate-200 bg-white p-6 shadow-sm">
          <h1 className="text-xl font-bold text-navy-900">{title}</h1>
          {subtitle && <p className="mt-1 text-sm text-slate-600">{subtitle}</p>}
          <div className="mt-5">{children}</div>
        </div>
      </main>
      <footer className="py-4 text-center text-xs text-slate-500">
        Open-source demo · fictional data
      </footer>
    </div>
  );
}
