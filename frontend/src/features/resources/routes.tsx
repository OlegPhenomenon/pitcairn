import type { RouteObject } from "react-router";
import { ResourcesPage } from "./ResourcesPage";
export const resourceRoutes: RouteObject[] = [
  { path: "admin/resources", element: <ResourcesPage /> },
];
