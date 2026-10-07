import type { RouteObject } from "react-router";
import { RequireRole } from "../auth/guards";
import { ReportsPage } from "./ReportsPage";
export const reportRoutes: RouteObject[] = [
  {
    path: "reports",
    element: (
      <RequireRole
        roles={[
          "coordinator",
          "decision_maker",
          "base_manager",
          "finance",
          "admin",
        ]}
      >
        <ReportsPage />
      </RequireRole>
    ),
  },
];
