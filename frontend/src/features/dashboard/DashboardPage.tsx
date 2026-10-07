import { Link } from 'react-router';
import { Plus } from 'lucide-react';

import { formatDate, toNum, titleize } from '../../lib/format';
import { Button, Card, CardBody, CardHeader, EmptyState, PageHeader, StatusBadge, Table } from '../../ui';
import { useMe } from '../auth/api';
import { useProjects } from '../projects/api';
import { useNotifications } from '../notifications/api';

/**
 * /app — interim dashboard: the user's projects + recent notifications.
 * The role-aware dashboard (§5 `GET /dashboard`) replaces this in a later slice.
 */
export function DashboardPage() {
  const me = useMe();
  const projects = useProjects(50);
  const notifications = useNotifications(8);

  return (
    <>
      <PageHeader
        title={`Welcome, ${me.data?.user.name ?? ''}`}
        subtitle="Your projects and what needs your attention."
        actions={
          <Link to="/app/projects/new">
            <Button icon={<Plus className="size-4" />}>New project</Button>
          </Link>
        }
      />

      <div className="grid gap-5 lg:grid-cols-3">
        <Card className="lg:col-span-2">
          <CardHeader title="Your projects" />
          <CardBody>
            <Table
              loading={projects.isPending}
              rows={projects.data?.items ?? []}
              rowKey={(p) => p.id}
              empty={{
                title: 'No projects yet',
                body: 'Start a new research application to see it here.',
              }}
              caption="Your projects"
              columns={[
                {
                  header: 'Title',
                  cell: (p) => (
                    <Link
                      to={`/app/projects/${p.id}`}
                      className="font-medium text-teal-800 hover:underline"
                    >
                      {p.title}
                    </Link>
                  ),
                },
                { header: 'Reference', cell: (p) => p.reference ?? '—' },
                { header: 'Status', cell: (p) => <StatusBadge status={p.status} /> },
                {
                  header: 'Your role',
                  cell: (p) => (p.my_role ? titleize(p.my_role) : '—'),
                  hideOnCard: true,
                },
                {
                  header: 'Created',
                  cell: (p) => formatDate(p.created_at),
                  hideOnCard: true,
                },
              ]}
            />
          </CardBody>
        </Card>

        <Card>
          <CardHeader title="Recent notifications" />
          <CardBody>
            {notifications.isPending ? (
              <p className="text-sm text-slate-500">Loading…</p>
            ) : (notifications.data?.items.length ?? 0) === 0 ? (
              <EmptyState title="No notifications" body="You're all caught up." />
            ) : (
              <ul className="flex flex-col gap-3">
                {notifications.data?.items.map((n) => (
                  <li key={n.id} className="text-sm">
                    <Link
                      to={n.link?.startsWith('/') ? n.link : '#'}
                      className="font-medium text-slate-900 hover:text-teal-800"
                    >
                      {n.title}
                    </Link>
                    <p className="text-xs text-slate-500">{formatDate(n.created_at)}</p>
                  </li>
                ))}
              </ul>
            )}
            {notifications.data && toNum(notifications.data.total) > 8 && (
              <p className="mt-3 text-xs text-slate-500">
                Showing the most recent — check the bell in the header for more.
              </p>
            )}
          </CardBody>
        </Card>
      </div>
    </>
  );
}
