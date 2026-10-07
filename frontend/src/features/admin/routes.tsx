import type { RouteObject } from 'react-router';
import { RequireRole } from '../auth/guards';
import { AdminUsersPage } from './UsersPage';
import { SettingsPage } from './SettingsPage';
import { JobsPage } from './JobsPage';
import { AuditPage } from './AuditPage';

export const adminRoutes: RouteObject[] = [{
  path: 'admin', element: <RequireRole roles={['admin']} />, children: [
    { path: 'users', element: <AdminUsersPage /> },
    { path: 'settings', element: <SettingsPage /> },
    { path: 'jobs', element: <JobsPage /> },
    { path: 'audit', element: <AuditPage /> },
  ],
}];
