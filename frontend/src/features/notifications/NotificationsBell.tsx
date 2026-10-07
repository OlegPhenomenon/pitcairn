import { useNavigate } from 'react-router';
import { Bell } from 'lucide-react';

import { formatDateTime, toNum } from '../../lib/format';
import { cx } from '../../lib/cx';
import { Dropdown } from '../../ui';
import { useMarkAllRead, useMarkRead, useNotifications } from './api';
import type { NotificationDto } from '../../api/types';

/** Header bell: unread count badge + dropdown + mark-all-read. */
export function NotificationsBell() {
  const navigate = useNavigate();
  const { data } = useNotifications(20);
  const markRead = useMarkRead();
  const markAll = useMarkAllRead();

  const items = data?.items ?? [];
  const unread = items.filter((n) => !n.read_at);

  const openItem = (n: NotificationDto) => {
    if (!n.read_at) markRead.mutate(n.id);
    if (n.link) {
      // Links are app-relative paths set by the backend.
      if (n.link.startsWith('/')) navigate(n.link);
      else window.open(n.link, '_blank', 'noopener');
    }
  };

  return (
    <Dropdown
      menuLabel="Notifications"
      trigger={(props) => (
        <button
          type="button"
          {...props}
          aria-label={`Notifications${unread.length ? `, ${unread.length} unread` : ''}`}
          className="relative rounded-md p-2 text-sand-100 hover:bg-navy-800 hover:text-white"
        >
          <Bell className="size-5" aria-hidden />
          {unread.length > 0 && (
            <span
              aria-hidden
              className="absolute -top-0.5 -right-0.5 flex h-5 min-w-5 items-center justify-center rounded-full bg-amber-400 px-1 text-[11px] font-bold text-navy-950"
            >
              {unread.length > 9 ? '9+' : unread.length}
            </span>
          )}
        </button>
      )}
    >
      {(close) => (
        <div className="w-80 max-w-[85vw]">
          <div className="flex items-center justify-between border-b border-slate-100 px-3 py-2">
            <span className="text-sm font-semibold text-slate-900">Notifications</span>
            {unread.length > 0 && (
              <button
                type="button"
                data-menu-item
                className="text-xs font-medium text-teal-700 hover:underline"
                onClick={() => {
                  markAll.mutate();
                  close();
                }}
              >
                Mark all read
              </button>
            )}
          </div>
          <div className="max-h-80 overflow-y-auto">
            {items.length === 0 && (
              <p className="px-3 py-6 text-center text-sm text-slate-500">
                No notifications yet.
              </p>
            )}
            {items.map((n) => (
              <button
                key={n.id}
                type="button"
                role="menuitem"
                data-menu-item
                onClick={() => {
                  openItem(n);
                  close();
                }}
                className={cx(
                  'block w-full border-b border-slate-50 px-3 py-2.5 text-left last:border-0 hover:bg-slate-50',
                  !n.read_at && 'bg-teal-50/50',
                )}
              >
                <span className="flex items-start gap-2">
                  {!n.read_at && (
                    <span
                      aria-hidden
                      className="mt-1.5 size-2 shrink-0 rounded-full bg-teal-600"
                    />
                  )}
                  <span className="min-w-0">
                    <span className="block truncate text-sm font-medium text-slate-900">
                      {n.title}
                    </span>
                    {n.body && (
                      <span className="mt-0.5 line-clamp-2 block text-xs text-slate-600">
                        {n.body}
                      </span>
                    )}
                    <span className="mt-0.5 block text-xs text-slate-400">
                      {formatDateTime(n.created_at)}
                    </span>
                  </span>
                </span>
              </button>
            ))}
          </div>
          {data && toNum(data.total) > items.length && (
            <p className="border-t border-slate-100 px-3 py-2 text-center text-xs text-slate-500">
              Showing the {items.length} most recent
            </p>
          )}
        </div>
      )}
    </Dropdown>
  );
}
