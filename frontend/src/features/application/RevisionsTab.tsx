import { useState } from 'react';
import { useApiQuery } from '../../api/client';
import type { ListResponse } from '../../api/types';
import type { RevisionDto } from '../../api/generated/RevisionDto';
import type { RevisionDiffDto } from '../../api/generated/RevisionDiffDto';
import { Banner, Card, CardBody, CardHeader, Select, StatusBadge } from '../../ui';
import { toNum } from '../../lib/format';
import { useProjectContext } from '../projects/projectContext';
import { DateText, QueryState } from './shared';
import { answerToText } from './schema';

export function RevisionsTab() {
  const { project } = useProjectContext();
  const revisions = useApiQuery<ListResponse<RevisionDto>>(['projects', project.id, 'revisions'], `/projects/${project.id}/revisions`);
  const [left, setLeft] = useState<number | null>(null);
  const [right, setRight] = useState<number | null>(null);
  const a = left ?? (revisions.data?.items.length ? toNum(revisions.data.items[0].number) : 0);
  const b = right ?? (revisions.data?.items.length ? toNum(revisions.data.items.at(-1)!.number) : 0);
  const diff = useApiQuery<RevisionDiffDto>(['projects', project.id, 'diff', a, b], `/projects/${project.id}/revisions/${b}/diff?against=${a}`, { enabled: a > 0 && b > 0 && a !== b });

  return <div className="space-y-5">
    <Card><CardHeader title="Submitted revisions" /><CardBody>
      <QueryState loading={revisions.isPending} error={revisions.error} empty={revisions.data?.items.length === 0}>
        <ul className="space-y-3">{revisions.data?.items.map(r => <li key={r.id} className="rounded-md border border-slate-200 p-3">
          <div className="flex flex-wrap gap-2"><strong>Revision {toNum(r.number)}</strong><StatusBadge status="submitted" /></div>
          <p className="text-sm text-slate-600">Submitted by {r.submitted_by_name} · <DateText value={r.submitted_at} time /> · Form v{toNum(r.template_version)}</p>
        </li>)}</ul>
      </QueryState>
    </CardBody></Card>
    {revisions.data && revisions.data.items.length > 1 && <Card><CardHeader title="Compare revisions" /><CardBody className="space-y-4">
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="text-sm font-medium">Earlier revision<Select value={a} onChange={e => setLeft(Number(e.target.value))}>{revisions.data.items.map(r => <option key={r.id} value={toNum(r.number)}>Revision {toNum(r.number)}</option>)}</Select></label>
        <label className="text-sm font-medium">Later revision<Select value={b} onChange={e => setRight(Number(e.target.value))}>{revisions.data.items.map(r => <option key={r.id} value={toNum(r.number)}>Revision {toNum(r.number)}</option>)}</Select></label>
      </div>
      {a === b ? <Banner tone="info">Choose two different revisions.</Banner> : <QueryState loading={diff.isPending} error={diff.error} empty={diff.data?.changes.length === 0}>
        <div className="space-y-3">{diff.data?.changes.map(change => <div key={change.path} className="rounded-md border border-slate-200 p-3">
          <p className="font-medium">{change.path} <StatusBadge status={change.kind} /></p>
          {change.redacted ? <p className="text-sm text-slate-600">Personal document details are hidden.</p> : <div className="mt-2 grid gap-2 sm:grid-cols-2">
            <div className="rounded bg-slate-50 p-2 text-sm"><strong>Before</strong><p className="break-words">{answerToText(change.before) || '—'}</p></div>
            <div className="rounded bg-teal-50 p-2 text-sm"><strong>After</strong><p className="break-words">{answerToText(change.after) || '—'}</p></div>
          </div>}
        </div>)}</div>
      </QueryState>}
    </CardBody></Card>}
  </div>;
}
