import type { RouteObject } from 'react-router';
import { RequireRole } from '../auth/guards';
import { AdminUsersPage } from './UsersPage';
import { SettingsPage } from './SettingsPage';
import { JobsPage } from './JobsPage';
import { AuditPage } from './AuditPage';
import { ROLE_MANAGERS } from './access';

export const adminRoutes: RouteObject[] = [
  {
    path: 'admin/users',
    element: (
      <RequireRole roles={ROLE_MANAGERS}>
        <AdminUsersPage />
      </RequireRole>
    ),
  },
  {
    path: 'admin', element: <RequireRole roles={['admin']} />, children: [
      { path: 'settings', element: <SettingsPage /> },
      { path: 'jobs', element: <JobsPage /> },
      { path: 'audit', element: <AuditPage /> },
    ],
  },
];
