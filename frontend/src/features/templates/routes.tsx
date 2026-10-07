import type { RouteObject } from "react-router";
import { RequireRole } from "../auth/guards";
import { TEMPLATE_EDITORS } from "../admin/access";
import { TemplatesPage } from "./TemplatesPage";
export const templateRoutes: RouteObject[] = [
  {
    path: "admin/templates",
    element: (
      <RequireRole roles={TEMPLATE_EDITORS}>
        <TemplatesPage />
      </RequireRole>
    ),
  },
];
