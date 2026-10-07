import type { RouteObject } from 'react-router';
import { DashboardPage } from './DashboardPage';

export const dashboardRoutes: RouteObject[] = [{ index: true, element: <DashboardPage /> }];
