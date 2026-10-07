import type { ReactNode } from 'react';
import { Link } from 'react-router';
import { ApiError } from '../../api/client';
import { formatDate, formatDateTime } from '../../lib/format';
import { Banner, EmptyState, Skeleton } from '../../ui';

export function QueryState({ loading, error, empty, children }: { loading: boolean; error: unknown; empty?: boolean; children: ReactNode }) {
  if (loading) return <div aria-busy="true"><Skeleton className="h-14 w-full" /><Skeleton className="mt-3 h-14 w-full" /></div>;
  if (error) return <Banner tone="error">{error instanceof ApiError ? error.message : 'Could not load this section.'}</Banner>;
  if (empty) return <EmptyState title="Nothing here yet" />;
  return children;
}

export function DateText({ value, time = false }: { value: string | null | undefined; time?: boolean }) {
  return value ? <time dateTime={value} title={value}>{time ? formatDateTime(value) : formatDate(value)}</time> : <span>—</span>;
}

export function SectionLink({ to, children }: { to: string; children: ReactNode }) {
  return <Link className="font-medium text-teal-700 underline-offset-2 hover:underline" to={to}>{children}</Link>;
}
