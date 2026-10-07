import type { ReactNode } from 'react';

import { cx } from '../lib/cx';
import { formatDateTime } from '../lib/format';
import { Badge, type BadgeTone } from './Badge';

export interface TimelineItem {
  id: string;
  at: string;
  title: ReactNode;
  body?: ReactNode;
  actor?: ReactNode;
  tone?: BadgeTone;
  badge?: ReactNode;
}

/** Vertical timeline (audit / project history). */
export function Timeline({ items }: { items: TimelineItem[] }) {
  return (
    <ol className="relative flex flex-col gap-0 border-l-2 border-slate-200 pl-5">
      {items.map((item) => (
        <li key={item.id} className="relative pb-5 last:pb-0">
          <span
            aria-hidden
            className={cx(
              'absolute top-1.5 -left-[26px] size-3 rounded-full ring-4 ring-white',
              item.tone === 'red'
                ? 'bg-red-500'
                : item.tone === 'green'
                  ? 'bg-green-600'
                  : item.tone === 'amber'
                    ? 'bg-amber-500'
                    : 'bg-slate-400',
            )}
          />
          <div className="flex flex-wrap items-baseline gap-x-2">
            <span className="text-sm font-medium text-slate-900">{item.title}</span>
            {item.badge && <Badge tone={item.tone ?? 'slate'}>{item.badge}</Badge>}
          </div>
          <div className="text-xs text-slate-500">
            {item.actor && <span>{item.actor} · </span>}
            <time dateTime={item.at}>{formatDateTime(item.at)}</time>
          </div>
          {item.body && <div className="mt-1 text-sm text-slate-600">{item.body}</div>}
        </li>
      ))}
    </ol>
  );
}
