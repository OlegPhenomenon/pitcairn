import { Link, Outlet, useOutletContext, useParams } from 'react-router';

import { ApiError } from '../../api/client';
import type { MeResponse, ProjectDto } from '../../api/types';
import {
  Banner,
  Card,
  CardBody,
  PageHeader,
  PageLoading,
  StatusBadge,
  Tabs,
} from '../../ui';
import { useMe } from '../auth/api';
import { useProject } from './api';

// Tabs from architecture §10. Only overview has real data in this slice;
// the rest render a neutral placeholder the later slices replace.
const TAB_DEFS = [
  { key: 'overview', label: 'Overview' },
  { key: 'application', label: 'Application' },
  { key: 'team', label: 'Team' },
  { key: 'messages', label: 'Messages' },
  { key: 'review', label: 'Review' },
  { key: 'decisions', label: 'Decisions' },
  { key: 'trips', label: 'Trips' },
  { key: 'invoices', label: 'Invoices' },
  { key: 'results', label: 'Results' },
  { key: 'samples', label: 'Samples' },
  { key: 'sites', label: 'Sites' },
  { key: 'history', label: 'History' },
  { key: 'changes', label: 'Changes' },
] as const;

export function ProjectLayout() {
  const { id } = useParams<{ id: string }>();
  const me = useMe();
  const project = useProject(id);

  if (project.isPending) return <PageLoading />;

  if (project.isError || !project.data) {
    const err = project.error;
    return (
      <Card>
        <CardBody>
          <Banner tone="error">
            {err instanceof ApiError && err.status === 404
              ? 'Project not found.'
              : err instanceof ApiError && err.status === 403
                ? 'You do not have access to this project.'
                : 'Could not load the project.'}
          </Banner>
          <p className="mt-3">
            <Link to="/app" className="text-teal-700 hover:underline">
              ← Back to dashboard
            </Link>
          </p>
        </CardBody>
      </Card>
    );
  }

  const p = project.data.project;
  const tabs = TAB_DEFS.map((t) => ({
    to: t.key === 'overview' ? `/app/projects/${p.id}` : `/app/projects/${p.id}/${t.key}`,
    label: t.label,
    end: t.key === 'overview',
  }));

  return (
    <>
      <PageHeader
        title={p.title}
        subtitle={
          <>
            {p.reference ?? 'Draft — no reference yet'} · {p.organisation || 'No organisation'}
            {me.data?.demo_mode && ' · demo'}
          </>
        }
        actions={<StatusBadge status={p.status} className="text-sm" />}
      />
      {project.data.primary_message && (
        <Banner tone="warning" className="mb-4">
          <strong>Needs your reply:</strong> {project.data.primary_message.title}
        </Banner>
      )}
      <Tabs tabs={tabs} ariaLabel="Project sections" />
      <div className="pt-5">
        <Outlet context={{ project: p, me: me.data } satisfies ProjectContext} />
      </div>
    </>
  );
}

interface ProjectContext {
  project: ProjectDto;
  me: MeResponse | undefined;
}

export function useProjectContext(): ProjectContext {
  return useOutletContext<ProjectContext>();
}

/** Neutral placeholder for tabs implemented by later slices. */
export function ComingSoonTab({ area }: { area: string }) {
  return (
    <Card>
      <CardBody>
        <h2 className="text-base font-semibold text-slate-800">Not available yet</h2>
        <p className="mt-1 text-sm text-slate-600">
          The {area} area arrives with a later backend slice. This tab will show it here.
        </p>
      </CardBody>
    </Card>
  );
}
