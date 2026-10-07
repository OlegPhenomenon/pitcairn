import type { RouteObject } from "react-router";
import { RequireRole } from "../auth/guards";
import { SearchPage } from "./SearchPage";
export const searchRoutes: RouteObject[] = [
  {
    path: "search",
    element: (
      <RequireRole
        roles={[
          "coordinator",
          "decision_maker",
          "base_manager",
          "finance",
          "admin",
          "expert",
        ]}
      >
        <SearchPage />
      </RequireRole>
    ),
  },
];
