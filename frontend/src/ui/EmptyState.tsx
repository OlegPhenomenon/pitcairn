import type { ReactNode } from 'react';
import { Inbox } from 'lucide-react';

import { cx } from '../lib/cx';

export function EmptyState({
  title,
  body,
  icon,
  action,
  className,
}: {
  title: string;
  body?: ReactNode;
  icon?: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cx(
        'flex flex-col items-center gap-2 rounded-lg border border-dashed border-slate-300 bg-slate-50/60 px-6 py-10 text-center',
        className,
      )}
    >
      <span className="text-slate-400" aria-hidden>
        {icon ?? <Inbox className="size-8" />}
      </span>
      <p className="text-sm font-semibold text-slate-700">{title}</p>
      {body && <div className="max-w-md text-sm text-slate-500">{body}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}
