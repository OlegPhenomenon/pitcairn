import type { RouteObject } from "react-router";
import { RequireRole } from "../auth/guards";
import { TemplatesPage } from "./TemplatesPage";
export const templateRoutes: RouteObject[] = [
  {
    path: "admin/templates",
    element: (
      <RequireRole roles={["admin"]}>
        <TemplatesPage />
      </RequireRole>
    ),
  },
];
