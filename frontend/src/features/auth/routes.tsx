import type { RouteObject } from 'react-router';

import { InvitePage } from './InvitePage';
import { LoginPage } from './LoginPage';
import { MfaEnrollPage } from './MfaEnrollPage';
import { MfaPage } from './MfaPage';
import { RegisterPage } from './RegisterPage';
import { DemoPage } from '../demo/DemoPage';
import { StoryPage } from '../story/StoryPage';

export const authRoutes: RouteObject[] = [
  { path: '/login', element: <LoginPage /> },
  { path: '/demo', element: <DemoPage /> },
  { path: '/demo/story', element: <main className="mx-auto max-w-5xl px-4 py-10"><StoryPage /></main> },
  { path: '/register', element: <RegisterPage /> },
  { path: '/mfa', element: <MfaPage /> },
  { path: '/mfa/enroll', element: <MfaEnrollPage /> },
  { path: '/invite/:token', element: <InvitePage /> },
];
