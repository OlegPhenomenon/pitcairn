import type { RouteObject } from "react-router";
import { CalendarPage } from "./CalendarPage";
export const calendarRoutes: RouteObject[] = [
  { path: "calendar", element: <CalendarPage /> },
];
