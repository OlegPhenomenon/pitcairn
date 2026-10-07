import type { RouteObject } from 'react-router';

import { InvitePage } from './InvitePage';
import { LoginPage } from './LoginPage';
import { MfaEnrollPage } from './MfaEnrollPage';
import { MfaPage } from './MfaPage';
import { RegisterPage } from './RegisterPage';

export const authRoutes: RouteObject[] = [
  { path: '/login', element: <LoginPage /> },
  { path: '/register', element: <RegisterPage /> },
  { path: '/mfa', element: <MfaPage /> },
  { path: '/mfa/enroll', element: <MfaEnrollPage /> },
  { path: '/invite/:token', element: <InvitePage /> },
];
