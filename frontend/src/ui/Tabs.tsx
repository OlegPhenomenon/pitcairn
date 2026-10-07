import { useRef } from 'react';
import { NavLink, useLocation } from 'react-router';

import { cx } from '../lib/cx';

export interface TabDef {
  to: string;
  label: string;
  end?: boolean;
}

/**
 * Router-linked tab bar. Arrow keys move between tabs (and activate them,
 * since focus follows NavLink). Each tab keeps its label visible and the
 * active one is marked with aria-current by NavLink.
 */
export function Tabs({ tabs, ariaLabel }: { tabs: TabDef[]; ariaLabel?: string }) {
  const listRef = useRef<HTMLDivElement>(null);
  const location = useLocation();

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft' && e.key !== 'Home' && e.key !== 'End')
      return;
    const items = Array.from(
      listRef.current?.querySelectorAll<HTMLAnchorElement>('a[role="tab"]') ?? [],
    );
    const idx = items.findIndex((el) => el === document.activeElement);
    if (idx === -1) return;
    e.preventDefault();
    let next = idx;
    if (e.key === 'ArrowRight') next = (idx + 1) % items.length;
    if (e.key === 'ArrowLeft') next = (idx - 1 + items.length) % items.length;
    if (e.key === 'Home') next = 0;
    if (e.key === 'End') next = items.length - 1;
    items[next].focus();
    items[next].click();
  };

  return (
    <div
      role="tablist"
      aria-label={ariaLabel}
      ref={listRef}
      onKeyDown={onKeyDown}
      className="-mb-px flex gap-1 overflow-x-auto border-b border-slate-200"
    >
      {tabs.map((t) => (
        <NavLink
          key={t.to}
          to={t.to}
          end={t.end}
          role="tab"
          aria-selected={location.pathname === t.to}
          className={({ isActive }) =>
            cx(
              'shrink-0 rounded-t-md border-b-2 px-3 py-2 text-sm font-medium whitespace-nowrap',
              isActive
                ? 'border-teal-700 bg-white text-teal-800'
                : 'border-transparent text-slate-600 hover:bg-slate-50 hover:text-slate-900',
            )
          }
        >
          {t.label}
        </NavLink>
      ))}
    </div>
  );
}
