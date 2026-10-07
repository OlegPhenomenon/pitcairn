import type { RouteObject } from "react-router";
import { RequireRole } from "../auth/guards";
import { TARIFF_EDITORS } from "../admin/access";
import { ResourcesPage } from "./ResourcesPage";
export const resourceRoutes: RouteObject[] = [
  {
    path: "admin/resources",
    element: (
      <RequireRole roles={TARIFF_EDITORS}>
        <ResourcesPage />
      </RequireRole>
    ),
  },
];
