import { createBrowserRouter } from 'react-router';
import { AppLayout } from './AppLayout';
import { ErrorPage, NotFoundPage } from './ErrorPage';
import { RequireAuth } from '../features/auth/guards';
import { LandingPage } from '../features/landing/LandingPage';
import { authRoutes } from '../features/auth/routes';
import { catalogRoutes } from '../features/catalog/routes';
import { dashboardRoutes } from '../features/dashboard/routes';
import { projectRoutes } from '../features/projects/routes';
import { adminRoutes } from '../features/admin/routes';
import { demoRoutes } from '../features/demo/routes';
import { reviewRoutes } from '../features/review/routes';

export const router = createBrowserRouter([
  { path: '/', element: <LandingPage />, errorElement: <ErrorPage /> },
  ...authRoutes,
  ...catalogRoutes,
  {
    path: '/app', element: <RequireAuth><AppLayout /></RequireAuth>, errorElement: <ErrorPage />,
    children: [
      ...dashboardRoutes,
      ...projectRoutes,
      ...reviewRoutes,
      ...adminRoutes,
      ...demoRoutes,
    ],
  },
  { path: '*', element: <NotFoundPage /> },
]);
