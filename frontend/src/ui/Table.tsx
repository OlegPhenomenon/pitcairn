import type { ReactNode } from 'react';

import { cx } from '../lib/cx';
import { EmptyState } from './EmptyState';
import { Skeleton } from './Skeleton';

export interface Column<T> {
  header: ReactNode;
  cell: (row: T) => ReactNode;
  /** Extra classes on the cell. */
  className?: string;
  /** In the mobile card layout, skip this column. */
  hideOnCard?: boolean;
}

/**
 * Responsive table: a real <table> on ≥md, stacked cards below that.
 */
export function Table<T>({
  columns,
  rows,
  rowKey,
  loading,
  empty,
  caption,
}: {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T) => string | number;
  loading?: boolean;
  empty?: { title: string; body?: string };
  caption?: string;
}) {
  if (loading) {
    return (
      <div className="flex flex-col gap-2" aria-busy="true" aria-label="Loading">
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full" />
      </div>
    );
  }
  if (rows.length === 0) {
    return <EmptyState title={empty?.title ?? 'Nothing here yet'} body={empty?.body} />;
  }
  return (
    <>
      <div className="overflow-x-auto max-md:hidden">
        <table className="w-full border-collapse text-left text-sm">
          {caption && <caption className="sr-only">{caption}</caption>}
          <thead>
            <tr className="border-b border-slate-200">
              {columns.map((c, i) => (
                <th
                  key={i}
                  scope="col"
                  className="px-3 py-2 text-xs font-semibold tracking-wide text-slate-500 uppercase"
                >
                  {c.header}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={rowKey(row)} className="border-b border-slate-100 last:border-0">
                {columns.map((c, i) => (
                  <td key={i} className={cx('px-3 py-2.5 align-top', c.className)}>
                    {c.cell(row)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <ul className="flex flex-col gap-3 md:hidden">
        {rows.map((row) => (
          <li
            key={rowKey(row)}
            className="rounded-md border border-slate-200 bg-white p-3 shadow-sm"
          >
            <dl className="flex flex-col gap-1.5">
              {columns
                .filter((c) => !c.hideOnCard)
                .map((c, i) => (
                  <div key={i} className="flex items-baseline justify-between gap-3">
                    <dt className="shrink-0 text-xs font-semibold tracking-wide text-slate-500 uppercase">
                      {c.header}
                    </dt>
                    <dd className="text-right text-sm text-slate-800">{c.cell(row)}</dd>
                  </div>
                ))}
            </dl>
          </li>
        ))}
      </ul>
    </>
  );
}
