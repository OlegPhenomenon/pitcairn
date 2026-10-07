import { useEffect, useRef, useState } from 'react';
import { Link, NavLink, Outlet, useLocation, useNavigate } from 'react-router';
import { useQueryClient } from '@tanstack/react-query';
import { Anchor, ChevronDown, FolderKanban, Globe2, LogOut, Mail, Menu, Settings2, X } from 'lucide-react';

import { cx } from '../lib/cx';
import { Dropdown, MenuItem, Badge } from '../ui';
import { useLogout, useMe } from '../features/auth/api';
import { NotificationsBell } from '../features/notifications/NotificationsBell';
import { PersonaSwitcher } from '../features/demo/PersonaSwitcher';
import type { MeResponse } from '../api/types';
import { ROLE_MANAGERS, TARIFF_EDITORS, TEMPLATE_EDITORS } from '../features/admin/access';

interface NavItem {
  to: string;
  label: string;
  icon?: React.ReactNode;
  end?: boolean;
}

function navItems(me: MeResponse): NavItem[] {
  const items: NavItem[] = [
    { to: '/app', label: 'Dashboard', icon: <FolderKanban className="size-4" />, end: true },
    { to: '/catalog', label: 'Catalog', icon: <Globe2 className="size-4" /> },
  ];
  if (me.user.roles.includes('expert')) items.push({ to: '/app/reviews', label: 'My reviews' });
  if (me.demo_mode) {
    items.push({ to: '/app/demo/story', label: 'Story guide' });
    items.push({ to: '/app/demo/mailbox', label: 'Demo mailbox', icon: <Mail className="size-4" /> });
    if (me.user.roles.includes('finance')) items.push({ to: '/app/demo/bank', label: 'Bank simulator' });
  }
  if (me.user.roles.some(r => ['base_manager', 'coordinator'].includes(r))) items.push({ to: '/app/calendar', label: 'Calendar' });
  if (me.user.roles.includes('provider')) items.push({ to: '/app/provider', label: 'Provider bookings' });
  if (me.user.roles.includes('finance')) items.push({ to: '/app/finance', label: 'Finance' });
  if (me.user.roles.some(r => ['coordinator','decision_maker','base_manager','finance','admin','expert'].includes(r))) items.push({ to: '/app/search', label: 'Search' });
  if (me.user.roles.some(r => ['coordinator','decision_maker','base_manager','finance','admin'].includes(r))) items.push({ to: '/app/reports', label: 'Reports' });
  return items;
}

// Settings screens, each shown only to the roles that may use it.
const ADMIN_ITEMS: (NavItem & { roles: string[] })[] = [
  { to: '/app/admin/templates', label: 'Templates', roles: TEMPLATE_EDITORS },
  { to: '/app/admin/import', label: 'Import', roles: ['admin'] },
  { to: '/app/admin/users', label: 'Users', roles: ROLE_MANAGERS },
  // Tariff editors include every resource editor.
  { to: '/app/admin/resources', label: 'Resources & tariffs', roles: TARIFF_EDITORS },
  { to: '/app/admin/settings', label: 'Settings', roles: ['admin'] },
  { to: '/app/admin/jobs', label: 'Jobs', roles: ['admin'] },
  { to: '/app/admin/audit', label: 'Audit log', roles: ['admin'] },
];

function useLogoutFlow() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const logout = useLogout();
  return async () => {
    try {
      await logout.mutateAsync();
    } finally {
      queryClient.clear();
      navigate('/', { replace: true });
    }
  };
}

export function AppLayout() {
  const me = useMe();
  const location = useLocation();
  const [drawerOpen, setDrawerOpen] = useState(false);
  const doLogout = useLogoutFlow();
  const drawerRef = useRef<HTMLDivElement>(null);

  // Close the drawer whenever the route changes (adjust state during render,
  // avoiding a cascading render from an effect).
  const [prevPath, setPrevPath] = useState(location.pathname);
  if (prevPath !== location.pathname) {
    setPrevPath(location.pathname);
    setDrawerOpen(false);
  }

  useEffect(() => {
    if (!drawerOpen) return;
    const onKey = (e: KeyboardEvent) => {
              if (e.key === 'Escape') setDrawerOpen(false);
              if (e.key === 'Tab' && drawerRef.current) {
                const items = Array.from(drawerRef.current.querySelectorAll<HTMLElement>('a[href], button:not([disabled])'));
                const first = items[0];
                const last = items[items.length - 1];
                if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last?.focus(); }
                else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first?.focus(); }
              }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [drawerOpen]);

  if (!me.data) return null;

  const user = me.data.user;
  const adminItems = ADMIN_ITEMS.filter((i) => i.roles.some((r) => user.roles.includes(r)));
  const menuLabel = user.roles.includes('admin') ? 'Admin' : 'Settings';
  const items = navItems(me.data);

  const linkClass = ({ isActive }: { isActive: boolean }) =>
    cx(
      'inline-flex items-center gap-1.5 rounded-md px-3 py-2 text-sm font-medium',
      isActive
        ? 'bg-navy-800 text-white'
        : 'text-sand-100 hover:bg-navy-800 hover:text-white',
    );

  return (
    <div className="flex min-h-dvh flex-col">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2 focus:z-50 focus:rounded focus:bg-white focus:px-3 focus:py-2 focus:text-navy-900"
      >
        Skip to content
      </a>

      <header className="bg-navy-900 text-white">
        <div className="mx-auto flex w-full max-w-6xl items-center gap-2 px-4 py-3">
          <button
            type="button"
            className="rounded-md p-2 text-sand-100 hover:bg-navy-800 md:hidden"
            aria-label="Open navigation menu"
            aria-expanded={drawerOpen}
            onClick={() => setDrawerOpen(true)}
          >
            <Menu className="size-5" aria-hidden />
          </button>
          <Link to="/" className="mr-2 inline-flex items-center gap-2 font-semibold">
            <Anchor className="size-5 text-teal-300" aria-hidden />
            <span className="hidden sm:inline">Pitcairn Research Hub</span>
            <span className="sm:hidden">Pitcairn</span>
          </Link>

          <nav aria-label="Main" className="hidden items-center gap-1 md:flex">
            {items.map((i) => (
              <NavLink key={i.to} to={i.to} end={i.end} className={linkClass}>
                {i.label}
              </NavLink>
            ))}
            {adminItems.length > 0 && (
              <Dropdown
                menuLabel="Administration"
                align="left"
                trigger={(props) => (
                  <button
                    type="button"
                    {...props}
                    className={cx(
                      'inline-flex items-center gap-1 rounded-md px-3 py-2 text-sm font-medium text-sand-100 hover:bg-navy-800 hover:text-white',
                      location.pathname.startsWith('/app/admin') && 'bg-navy-800 text-white',
                    )}
                  >
                    <Settings2 className="size-4" aria-hidden />
                    {menuLabel}
                    <ChevronDown className="size-3.5" aria-hidden />
                  </button>
                )}
              >
                {(close) => adminItems.map((i) => (
                  <NavLink
                    key={i.to}
                    to={i.to}
                    role="menuitem"
                    onClick={close}
                    className="block w-full px-3 py-2 text-sm text-slate-800 hover:bg-slate-50"
                  >
                    {i.label}
                  </NavLink>
                ))}
              </Dropdown>
            )}
          </nav>

          <div className="ml-auto flex items-center gap-1.5">
            <PersonaSwitcher demoMode={me.data.demo_mode} />
            <NotificationsBell />
            <Dropdown
              menuLabel="User menu"
              trigger={(props) => (
                <button
                  type="button"
                  {...props}
                  className="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-sand-100 hover:bg-navy-800 hover:text-white"
                >
                  <span
                    aria-hidden
                    className="flex size-7 items-center justify-center rounded-full bg-teal-700 text-xs font-bold text-white"
                  >
                    {user.name.slice(0, 1).toUpperCase()}
                  </span>
                  <span className="hidden max-w-40 truncate sm:inline">{user.name}</span>
                  <ChevronDown className="hidden size-3.5 sm:block" aria-hidden />
                </button>
              )}
            >
              <div className="border-b border-slate-100 px-3 py-2">
                <p className="text-sm font-semibold text-slate-900">{user.name}</p>
                <p className="truncate text-xs text-slate-500">{user.email}</p>
                <p className="truncate text-xs text-slate-500">{user.organisation}</p>
                {user.roles.length > 0 && (
                  <p className="mt-1 flex flex-wrap gap-1">
                    {user.roles.map((r) => (
                      <Badge key={r} tone="teal">
                        {r.replaceAll('_', ' ')}
                      </Badge>
                    ))}
                  </p>
                )}
              </div>
              <MenuItem onClick={() => void doLogout()} danger>
                <LogOut className="size-4" aria-hidden /> Log out
              </MenuItem>
            </Dropdown>
          </div>
        </div>

        {/* Mobile drawer */}
        {drawerOpen && (
          <div
            className="fixed inset-0 z-40 bg-navy-950/50 md:hidden"
            onMouseDown={(e) => {
              if (e.target === e.currentTarget) setDrawerOpen(false);
            }}
          >
            <div
              ref={drawerRef}
              role="dialog"
              aria-modal="true"
              aria-label="Navigation menu"
              className="h-full w-72 max-w-[85vw] overflow-y-auto bg-navy-900 p-4 text-white shadow-xl"
            >
              <div className="mb-4 flex items-center justify-between">
                <span className="inline-flex items-center gap-2 font-semibold">
                  <Anchor className="size-5 text-teal-300" aria-hidden />
                  Pitcairn Research Hub
                </span>
                <button
                  type="button"
                  aria-label="Close navigation menu"
                  className="rounded-md p-2 hover:bg-navy-800"
                  onClick={() => setDrawerOpen(false)}
                  autoFocus
                >
                  <X className="size-5" aria-hidden />
                </button>
              </div>
              <nav aria-label="Mobile" className="flex flex-col gap-1">
                {items.map((i) => (
                  <NavLink key={i.to} to={i.to} end={i.end} className={linkClass}>
                    {i.icon}
                    {i.label}
                  </NavLink>
                ))}
                {adminItems.length > 0 && (
                  <>
                    <p className="mt-3 px-3 text-xs font-semibold tracking-wide text-navy-300 uppercase">
                      {menuLabel}
                    </p>
                    {adminItems.map((i) => (
                      <NavLink key={i.to} to={i.to} className={linkClass}>
                        {i.label}
                      </NavLink>
                    ))}
                  </>
                )}
              </nav>
            </div>
          </div>
        )}
      </header>

      {me.data.demo_mode && (
        <div className="border-b border-teal-200 bg-teal-50 px-4 py-2 text-center text-sm text-teal-950">
          Demo — fictional people and data. Do not upload real applications or personal
          documents. <Link to="/app/demo/story" className="font-semibold underline">Open the story guide</Link>.
        </div>
      )}

      <main id="main" className="mx-auto w-full max-w-6xl flex-1 px-4 py-6">
        <Outlet />
      </main>

      <footer className="border-t border-slate-200 py-4 text-center text-xs text-slate-500">
        Open-source demo · fictional data
      </footer>
    </div>
  );
}
