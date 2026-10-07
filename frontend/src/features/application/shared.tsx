import type { ReactNode } from 'react';
import { Link } from 'react-router';
import { ApiError } from '../../api/client';
import { formatDate, formatDateTime } from '../../lib/format';
import { Banner, EmptyState, Skeleton } from '../../ui';
import { useProjectContext } from '../projects/ProjectLayout';

export function usePermissions() {
  const { project, me } = useProjectContext();
  const roles = me?.user.roles ?? [];
  const access = project.my_access;
  return {
    canEdit: (access === 'team_editor' || access === 'team_lead') && (project.status === 'draft' || project.status === 'changes_requested'),
    teamEditor: access === 'team_editor' || access === 'team_lead',
    lead: access === 'team_lead',
    coordinator: roles.includes('coordinator'),
    decisionMaker: roles.includes('decision_maker'),
    baseManager: roles.includes('base_manager'),
    expert: roles.includes('expert') && access === 'expert',
    canComment: roles.includes('coordinator') || roles.includes('decision_maker') || roles.includes('base_manager') || (roles.includes('expert') && access === 'expert'),
    staff: access === 'staff' || access === 'expert',
  };
}

export function QueryState({ loading, error, empty, children }: { loading: boolean; error: unknown; empty?: boolean; children: ReactNode }) {
  if (loading) return <div aria-busy="true"><Skeleton className="h-14 w-full" /><Skeleton className="mt-3 h-14 w-full" /></div>;
  if (error) return <Banner tone="error">{error instanceof ApiError ? error.message : 'Could not load this section.'}</Banner>;
  if (empty) return <EmptyState title="Nothing here yet" />;
  return children;
}

export function DateText({ value, time = false }: { value: string | null | undefined; time?: boolean }) {
  return value ? <time dateTime={value} title={value}>{time ? formatDateTime(value) : formatDate(value)}</time> : <span>—</span>;
}

export function SectionLink({ to, children }: { to: string; children: ReactNode }) {
  return <Link className="font-medium text-teal-700 underline-offset-2 hover:underline" to={to}>{children}</Link>;
}
