import type { RouteObject } from 'react-router';
import { CatalogPage } from './CatalogPage';
import { CatalogDetailPage } from './CatalogDetailPage';

export const catalogRoutes: RouteObject[] = [
  { path: '/catalog', element: <CatalogPage /> },
  { path: '/catalog/:reference', element: <CatalogDetailPage /> },
];
