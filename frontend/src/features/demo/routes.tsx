import type { RouteObject } from 'react-router';
import { MailboxPage } from './MailboxPage';

export const demoRoutes: RouteObject[] = [{ path: 'demo/mailbox', element: <MailboxPage /> }];
