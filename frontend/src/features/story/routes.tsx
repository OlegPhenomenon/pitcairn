import type { RouteObject } from "react-router";
import { StoryPage } from "./StoryPage";
export const storyRoutes: RouteObject[] = [
  { path: "demo/story", element: <StoryPage /> },
];
