import { useMemo, useState } from 'react';
import { useApiQuery } from '../../api/client';
import type { ListResponse } from '../../api/types';
import type { AuditEventDto } from '../../api/generated/AuditEventDto';
import { Card, CardBody, CardHeader, Select } from '../../ui';
import { useProjectContext } from '../projects/projectContext';
import { DateText, QueryState } from '../application/shared';

export function HistoryTab() {
  const { project } = useProjectContext();
  const events = useApiQuery<ListResponse<AuditEventDto>>(['projects', project.id, 'timeline'], `/projects/${project.id}/timeline`);
  const [filter, setFilter] = useState('all');
  const types = useMemo(() => [...new Set(events.data?.items.map(e => e.entity_type) ?? [])].sort(), [events.data]);
  const shown = events.data?.items.filter(e => filter === 'all' || e.entity_type === filter) ?? [];
  return <Card><CardHeader title="Project history" /><CardBody className="space-y-4"><label className="block max-w-xs text-sm font-medium">Filter by type<Select value={filter} onChange={e => setFilter(e.target.value)}><option value="all">All events</option>{types.map(type => <option key={type}>{type}</option>)}</Select></label><QueryState loading={events.isPending} error={events.error} empty={shown.length === 0}><ol className="relative border-l border-slate-200 pl-5">{shown.map(e => <li key={e.id} className="relative pb-5 before:absolute before:-left-[25px] before:top-1 before:size-2 before:rounded-full before:bg-teal-700"><p className="font-medium">{e.summary}</p><p className="text-sm text-slate-600">{e.actor_label} · <DateText value={e.at} time /> · {e.entity_type}</p>{e.reason && <p className="mt-1 text-sm">Reason: {e.reason}</p>}</li>)}</ol></QueryState></CardBody></Card>;
}
