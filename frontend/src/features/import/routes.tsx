import type { RouteObject } from "react-router";
import { RequireRole } from "../auth/guards";
import { ImportPage } from "./ImportPage";
export const importRoutes: RouteObject[] = [
  {
    path: "admin/import",
    element: (
      <RequireRole roles={["admin"]}>
        <ImportPage />
      </RequireRole>
    ),
  },
];
