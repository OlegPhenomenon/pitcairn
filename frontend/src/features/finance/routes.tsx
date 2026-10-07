import type { RouteObject } from "react-router";
import { FinancePage } from "./FinancePage";
export const financeRoutes: RouteObject[] = [
  { path: "finance", element: <FinancePage /> },
];
