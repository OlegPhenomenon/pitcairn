import type { RouteObject } from 'react-router';

import { NewProjectPage } from './NewProjectPage';
import { OverviewTab } from './OverviewTab';
import { ComingSoonTab, ProjectLayout } from './ProjectLayout';
import { TripsTab } from '../trips/TripsTab';
import { InvoicesTab } from '../finance/InvoicesTab';

const comingSoon = (area: string) => <ComingSoonTab area={area} />;

export const projectRoutes: RouteObject[] = [
  { path: 'projects/new', element: <NewProjectPage /> },
  {
    path: 'projects/:id',
    element: <ProjectLayout />,
    children: [
      { index: true, element: <OverviewTab /> },
      { path: 'overview', element: <OverviewTab /> },
      { path: 'application', element: comingSoon('application form') },
      { path: 'team', element: comingSoon('team') },
      { path: 'messages', element: comingSoon('messages') },
      { path: 'review', element: comingSoon('expert review') },
      { path: 'decisions', element: comingSoon('decisions') },
      { path: 'trips', element: <TripsTab /> },
      { path: 'invoices', element: <InvoicesTab /> },
      { path: 'results', element: comingSoon('deliverables and results') },
      { path: 'samples', element: comingSoon('samples') },
      { path: 'sites', element: comingSoon('sites map') },
      { path: 'history', element: comingSoon('project history') },
      { path: 'changes', element: comingSoon('change requests') },
    ],
  },
];
