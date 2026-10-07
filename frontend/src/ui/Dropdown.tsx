import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from 'react';

import { cx } from '../lib/cx';

/**
 * A minimal accessible dropdown: button + absolutely-positioned panel.
 * Esc closes and refocuses the button; Arrow keys move between items;
 * Tab passes through naturally (and closes); click-outside closes.
 * Items inside should be buttons/links with role="menuitem" or [data-menu-item].
 */
export function Dropdown({
  trigger,
  children,
  align = 'right',
  className,
  menuLabel,
}: {
  trigger: (props: {
    id: string;
    'aria-expanded': boolean;
    'aria-haspopup': true;
    onClick: () => void;
    onKeyDown: (e: ReactKeyboardEvent) => void;
  }) => ReactNode;
  children: ReactNode | ((close: () => void) => ReactNode);
  align?: 'left' | 'right';
  className?: string;
  menuLabel?: string;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const menuId = useId();
  const close = () => setOpen(false);

  useEffect(() => {
    if (!open) return;
    const onDocMouseDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDocMouseDown);
    return () => document.removeEventListener('mousedown', onDocMouseDown);
  }, [open]);

  const focusItem = (dir: 1 | -1 | 'first' | 'last') => {
    // Looked up by id (not a ref) so this can run from deferred callbacks.
    const menu = document.getElementById(menuId);
    const items = Array.from(
      menu?.querySelectorAll<HTMLElement>(
        '[data-menu-item], [role="menuitem"], a[href], button:not([disabled])',
      ) ?? [],
    );
    if (items.length === 0) return;
    const idx = items.findIndex((el) => el === document.activeElement);
    let next = 0;
    if (dir === 'first') next = 0;
    else if (dir === 'last') next = items.length - 1;
    else next = (idx + dir + items.length) % items.length;
    items[next].focus();
  };

  const onMenuKeyDown = (e: ReactKeyboardEvent) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      setOpen(false);
      rootRef.current?.querySelector<HTMLElement>('button')?.focus();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      focusItem(1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      focusItem(-1);
    } else if (e.key === 'Home') {
      e.preventDefault();
      focusItem('first');
    } else if (e.key === 'End') {
      e.preventDefault();
      focusItem('last');
    }
  };

  return (
    <div ref={rootRef} className={cx('relative', className)}>
      {trigger({
        id: menuId,
        'aria-expanded': open,
        'aria-haspopup': true,
        onClick: () => setOpen((o) => !o),
        onKeyDown: (e) => {
          if (e.key !== 'ArrowDown') return;
          e.preventDefault();
          const wasOpen = open;
          if (!wasOpen) setOpen(true);
          // Defer so the menu panel is mounted before we touch refs.
          window.setTimeout(() => focusItem(wasOpen ? 1 : 'first'), 0);
        },
      })}
      {open && (
        <div
          id={menuId}
          role="menu"
          aria-label={menuLabel}
          onKeyDown={onMenuKeyDown}
          className={cx(
            'absolute z-30 mt-1 min-w-52 overflow-hidden rounded-md border border-slate-200 bg-white py-1 shadow-lg',
            align === 'right' ? 'right-0' : 'left-0',
          )}
        >
          {typeof children === 'function' ? children(close) : children}
        </div>
      )}
    </div>
  );
}

export function MenuItem({
  children,
  onClick,
  className,
  danger,
}: {
  children: ReactNode;
  onClick?: () => void;
  className?: string;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      data-menu-item
      onClick={onClick}
      className={cx(
        'flex w-full items-center gap-2 px-3 py-2 text-left text-sm hover:bg-slate-50',
        danger ? 'text-red-700' : 'text-slate-800',
        className,
      )}
    >
      {children}
    </button>
  );
}
