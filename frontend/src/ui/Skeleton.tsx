import { cx } from '../lib/cx';

export function Skeleton({ className }: { className?: string }) {
  return (
    <div
      aria-hidden
      className={cx('rounded-md bg-slate-200/80 motion-safe:animate-pulse', className)}
    />
  );
}

/** Full-page loading placeholder with an accessible label. */
export function PageLoading() {
  return (
    <div className="flex flex-col gap-4 p-6" role="status" aria-label="Loading">
      <span className="sr-only">Loading…</span>
      <Skeleton className="h-8 w-48" />
      <Skeleton className="h-40 w-full" />
      <Skeleton className="h-24 w-full" />
    </div>
  );
}
