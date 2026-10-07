import type { RouteObject } from "react-router";
import { BankPage } from "./BankPage";
export const bankRoutes: RouteObject[] = [
  { path: "demo/bank", element: <BankPage /> },
];
