import type { RouteObject } from 'react-router';
import { MyReviewsPage } from './ReviewTab';
export const reviewRoutes: RouteObject[] = [{ path: 'reviews', element: <MyReviewsPage /> }];
