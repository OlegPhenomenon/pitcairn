import type { RouteObject } from 'react-router';

import { NewProjectPage } from './NewProjectPage';
import { OverviewTab } from '../application/OverviewTab';
import { ApplicationTab } from '../application/ApplicationTab';
import { RevisionsTab } from '../application/RevisionsTab';
import { TeamTab } from '../team/TeamTab';
import { MessagesTab } from '../conversation/MessagesTab';
import { ReviewTab } from '../review/ReviewTab';
import { DecisionsTab } from '../decisions/DecisionsTab';
import { ChangesTab } from '../changes/ChangesTab';
import { HistoryTab } from '../history/HistoryTab';
import { SitesTab } from '../sites/SitesTab';
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
      { path: 'application', element: <ApplicationTab /> },
      { path: 'revisions', element: <RevisionsTab /> },
      { path: 'team', element: <TeamTab /> },
      { path: 'messages', element: <MessagesTab /> },
      { path: 'review', element: <ReviewTab /> },
      { path: 'decisions', element: <DecisionsTab /> },
      { path: 'trips', element: <TripsTab /> },
      { path: 'invoices', element: <InvoicesTab /> },
      { path: 'results', element: comingSoon('deliverables and results') },
      { path: 'samples', element: comingSoon('samples') },
      { path: 'sites', element: <SitesTab /> },
      { path: 'history', element: <HistoryTab /> },
      { path: 'changes', element: <ChangesTab /> },
    ],
  },
];
