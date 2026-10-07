import type { ReactNode } from 'react';

import { cx } from '../lib/cx';

export interface KeyValueItem {
  key: ReactNode;
  value: ReactNode;
}

/** Definition list for "label: value" displays. */
export function KeyValue({ items, className }: { items: KeyValueItem[]; className?: string }) {
  return (
    <dl className={cx('grid grid-cols-1 gap-x-6 gap-y-3 sm:grid-cols-2', className)}>
      {items.map((item, i) => (
        <div key={i} className="min-w-0">
          <dt className="text-xs font-semibold tracking-wide text-slate-500 uppercase">
            {item.key}
          </dt>
          <dd className="mt-0.5 text-sm break-words text-slate-900">{item.value}</dd>
        </div>
      ))}
    </dl>
  );
}
