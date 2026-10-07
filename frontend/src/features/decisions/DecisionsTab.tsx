import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { apiPatch, apiPost, fieldError, useApiQuery } from '../../api/client';
import type { ListResponse } from '../../api/types';
import type { CreateDecisionRequest } from '../../api/generated/CreateDecisionRequest';
import type { PatchDecisionRequest } from '../../api/generated/PatchDecisionRequest';
import type { DecisionDto } from '../../api/generated/DecisionDto';
import type { RevisionDto } from '../../api/generated/RevisionDto';
import { Banner, Button, Card, CardBody, CardHeader, Dialog, FormField, Input, Select, StatusBadge, Textarea, useToast } from '../../ui';
import { useProjectContext } from '../projects/projectContext';
import { useProjectDocuments } from '../projects/api';
import { DocumentSlot } from '../upload/DocumentSlot';
import { DateText, QueryState } from '../application/shared';
import { usePermissions } from '../application/permissions';
import { decisionKindLabel } from '../../lib/decisionKind';
import { issuedRefusal, permitChainHeads, permitName, permitsInForce } from '../../lib/permits';

const lines = (text: string) => text.split('\n').map(s => s.trim()).filter(Boolean);
const DRAFT_ID = 'decision-draft';

function Terms({ d }: { d: DecisionDto }) {
  return <>
    <p className="text-sm">Valid <DateText value={d.valid_from} /> to <DateText value={d.valid_to} /> · Legal reference: {d.legal_reference || '—'}</p>
    <p className="text-sm">{d.basis}</p>
    {d.permitted_activities.length > 0 && <><h4 className="text-sm font-semibold">Permitted activities</h4><ul className="list-disc pl-5 text-sm">{d.permitted_activities.map((s, i) => <li key={i}>{s}</li>)}</ul></>}
    {d.conditions.length > 0 && <><h4 className="text-sm font-semibold">Conditions</h4><ul className="list-disc pl-5 text-sm">{d.conditions.map((s, i) => <li key={i}>{s}</li>)}</ul></>}
    {d.restrictions.length > 0 && <><h4 className="text-sm font-semibold">Restrictions</h4><ul className="list-disc pl-5 text-sm">{d.restrictions.map((s, i) => <li key={i}>{s}</li>)}</ul></>}
    <p className="text-sm">Sites covered: {d.sites.length} · Issued by {d.issued_by_name ?? '—'} · <DateText value={d.issued_at} time /></p>
    <a className="text-sm text-teal-700 underline" href={`/api/v1/decisions/${d.id}/document`} target="_blank" rel="noreferrer">Open printable document</a>
  </>;
}

export function DecisionsTab() {
  const { project, me } = useProjectContext(); const rights = usePermissions(); const query = useQueryClient(); const toast = useToast();
  const key = ['projects', project.id, 'decisions']; const decisions = useApiQuery<ListResponse<DecisionDto>>(key, `/projects/${project.id}/decisions`);
  const revisions = useApiQuery<ListResponse<RevisionDto>>(['projects', project.id, 'revisions'], `/projects/${project.id}/revisions`, { enabled: rights.coordinator || rights.decisionMaker });
  const docs = useProjectDocuments(project.id);
  const [kind, setKind] = useState('permit'); const [title, setTitle] = useState(''); const [revision, setRevision] = useState(''); const [basis, setBasis] = useState(''); const [legal, setLegal] = useState('Marine Conservation Regulations 2022'); const [from, setFrom] = useState(''); const [to, setTo] = useState(''); const [activities, setActivities] = useState(''); const [conditions, setConditions] = useState(''); const [restrictions, setRestrictions] = useState(''); const [supersedes, setSupersedes] = useState(''); const [signed, setSigned] = useState(''); const [issue, setIssue] = useState<DecisionDto | null>(null); const [editingId, setEditingId] = useState<string | null>(null); const [busy, setBusy] = useState(false); const [error, setError] = useState<unknown>(null);
  async function run(action: () => Promise<unknown>, message: string) { setBusy(true); setError(null); try { await action(); toast.success(message); await query.invalidateQueries({ queryKey: key }); await query.invalidateQueries({ queryKey: ['projects', project.id] }); } catch (e) { setError(e); toast.error('Could not save decision', e instanceof Error ? e.message : undefined); } finally { setBusy(false); } }
  const items = decisions.data?.items ?? [];
  const byId = new Map(items.map(d => [d.id, d]));
  const heads = permitChainHeads(items);
  const inForce = permitsInForce(items);
  const refusal = issuedRefusal(items);
  const canDraft = rights.coordinator || rights.decisionMaker;
  const canIssue = (d: DecisionDto) => d.kind === 'refusal' ? project.status === 'in_review' : d.kind === 'permit' && !d.supersedes_id ? ['in_review', 'approved'].includes(project.status) : project.status === 'approved';
  function loadDraft(d: DecisionDto) { setEditingId(d.id); setKind(d.kind); setTitle(d.title); setRevision(d.project_revision_id); setBasis(d.basis); setLegal(d.legal_reference); setFrom(d.valid_from ?? ''); setTo(d.valid_to ?? ''); setActivities(d.permitted_activities.join('\n')); setConditions(d.conditions.join('\n')); setRestrictions(d.restrictions.join('\n')); setSupersedes(d.supersedes_id ?? ''); setSigned(d.document_version_id ?? ''); }
  function clearDraft() { setEditingId(null); setKind('permit'); setTitle(''); setRevision(''); setBasis(''); setLegal('Marine Conservation Regulations 2022'); setFrom(''); setTo(''); setActivities(''); setConditions(''); setRestrictions(''); setSupersedes(''); setSigned(''); }
  /** Prefill a draft that changes exactly one permit chain. */
  function startChange(target: DecisionDto, change: 'amendment' | 'extension' | 'revocation') {
    clearDraft(); setKind(change); setSupersedes(target.id); setRevision(target.project_revision_id); setLegal(target.legal_reference);
    if (change !== 'revocation') { setFrom(target.valid_from ?? ''); setTo(target.valid_to ?? ''); setActivities(target.permitted_activities.join('\n')); setConditions(target.conditions.join('\n')); setRestrictions(target.restrictions.join('\n')); }
    document.getElementById(DRAFT_ID)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }
  return <div className="space-y-5">{Boolean(error) && <Banner tone="error">{error instanceof Error ? error.message : 'Could not save decision.'}</Banner>}
    {refusal && <Card><CardHeader title="Application refused" actions={<StatusBadge status="refused" />} /><CardBody className="space-y-2"><Terms d={refusal} /></CardBody></Card>}
    {heads.length > 0 && <Card><CardHeader title={`Permits in force (${inForce.length})`} /><CardBody className="space-y-4">
      <p className="text-sm text-slate-600">Each permit is a separate decision. Amending, extending or revoking one permit does not change the others.</p>
      {heads.map(d => <section key={d.id} aria-label={`Permit ${permitName(d)}`} className="space-y-2 rounded-md border border-slate-200 p-3">
        <div className="flex flex-wrap items-center gap-2"><h3 className="font-semibold">Permit: {permitName(d)}</h3>{d.kind !== 'permit' && <StatusBadge status="info" label={`Latest: ${decisionKindLabel(d.kind)}`} />}{d.kind === 'revocation' ? <StatusBadge status="refused" label="Revoked" /> : <StatusBadge status="active" label="In force" />}</div>
        <Terms d={d} />
        {canDraft && d.kind !== 'revocation' && project.status === 'approved' && <div className="flex flex-wrap gap-2 pt-1">{(['amendment', 'extension', 'revocation'] as const).map(change => <Button key={change} size="sm" variant="secondary" onClick={() => startChange(d, change)}>{change === 'amendment' ? 'Amend' : change === 'extension' ? 'Extend' : 'Revoke'} this permit</Button>)}</div>}
      </section>)}
    </CardBody></Card>}
    <Card><CardHeader title="Decision history" /><CardBody><QueryState loading={decisions.isPending} error={decisions.error} empty={items.length === 0}><ul className="space-y-3">{items.map(d => { const replaced = d.supersedes_id ? byId.get(d.supersedes_id) : undefined; return <li key={d.id} className="rounded-md border border-slate-200 p-3"><div className="flex flex-wrap gap-2"><strong>{decisionKindLabel(d.kind)}</strong>{d.title && d.title !== decisionKindLabel(d.kind) && <span>· {d.title}</span>}<StatusBadge status={d.status} />{d.superseded_by_id && <StatusBadge status="superseded" />}{heads.some(h => h.id === d.id) && d.kind !== 'revocation' && <StatusBadge status="active" label="In force" />}</div><p className="text-sm">Revision {String(d.revision_number)} · {d.issued_by_name ? `Issued by ${d.issued_by_name}` : `Drafted by ${d.drafted_by_name}`} · <DateText value={d.issued_at ?? d.created_at} time /></p>{replaced && <p className="text-sm text-slate-600">Replaces {decisionKindLabel(replaced.kind)}{permitName(replaced) !== decisionKindLabel(replaced.kind) && ` “${permitName(replaced)}”`} issued <DateText value={replaced.issued_at} /> (kept in history)</p>}{d.status === 'issued' && <a className="text-sm text-teal-700 underline" href={`/api/v1/decisions/${d.id}/document`} target="_blank" rel="noreferrer">Open printable document</a>}{canDraft && d.status === 'draft' && <Button size="sm" variant="secondary" onClick={() => loadDraft(d)}>Edit draft</Button>}{rights.decisionMaker && d.status === 'draft' && canIssue(d) && <Button size="sm" loading={busy} onClick={() => setIssue(d)}>Issue decision</Button>}</li>; })}</ul></QueryState></CardBody></Card>
    {canDraft && <div id={DRAFT_ID}><Card><CardHeader title={editingId ? 'Edit decision draft' : 'Draft a decision'} /><CardBody className="space-y-3">
      <FormField label="Kind" error={fieldError(error, 'kind')}><Select value={kind} onChange={e => setKind(e.target.value)}>{['permit', 'refusal', 'amendment', 'extension', 'revocation'].map(k => <option key={k} value={k}>{decisionKindLabel(k)}</option>)}</Select></FormField>
      {kind !== 'refusal' && <FormField label="Permit name" help={kind === 'permit' ? 'Names this permit, e.g. “Reef transect sampling”. A project can hold several permits.' : 'Leave empty to keep the name of the permit being changed.'} error={fieldError(error, 'title')}><Input value={title} onChange={e => setTitle(e.target.value)} /></FormField>}
      {kind !== 'refusal' && inForce.length > 0 && <FormField label={kind === 'permit' ? 'Replaces permit (optional)' : 'Permit this decision changes'} help="Only the selected permit changes; the other permits stay in force." error={fieldError(error, 'supersedes_id')}><Select value={supersedes} onChange={e => setSupersedes(e.target.value)}><option value="">{kind === 'permit' ? 'None: a new, separate permit' : 'Choose permit'}</option>{inForce.map(h => <option key={h.id} value={h.id}>{permitName(h)} · {decisionKindLabel(h.kind)} · valid to {h.valid_to ?? '—'}</option>)}</Select></FormField>}
      <FormField label="Application revision" error={fieldError(error, 'project_revision_id')}><Select value={revision} onChange={e => setRevision(e.target.value)}><option value="">Choose revision</option>{revisions.data?.items.map(r => <option key={r.id} value={r.id}>Revision {String(r.number)}</option>)}</Select></FormField>
      <FormField label="Basis" error={fieldError(error, 'basis')}><Textarea value={basis} onChange={e => setBasis(e.target.value)} /></FormField>
      <FormField label="Legal reference" error={fieldError(error, 'legal_reference')}><Input list="legal-references" value={legal} onChange={e => setLegal(e.target.value)} /><datalist id="legal-references"><option value="Marine Conservation Regulations 2022" /><option value="Marine Science Base User Agreement (Annex 1)" /></datalist></FormField>
      <div className="grid gap-3 sm:grid-cols-2"><FormField label="Valid from" error={fieldError(error, 'valid_from')}><Input type="date" value={from} onChange={e => setFrom(e.target.value)} /></FormField><FormField label="Valid to" error={fieldError(error, 'valid_to')}><Input type="date" value={to} onChange={e => setTo(e.target.value)} /></FormField></div>
      {([['Permitted activities', activities, setActivities], ['Conditions', conditions, setConditions], ['Restrictions', restrictions, setRestrictions]] as const).map(([label, value, setter]) => <FormField key={label} label={`${label} (one per line)`} error={fieldError(error, label.toLowerCase().replaceAll(' ', '_'))}><Textarea value={value} onChange={e => setter(e.target.value)} /></FormField>)}
      <p className="text-sm text-slate-600">Sites are copied from the project when this decision is issued.</p>
      <FormField label="Signed copy (optional)"><Select value={signed} onChange={e => setSigned(e.target.value)}><option value="">No signed copy</option>{docs.data?.items.filter(d => d.category === 'decision' && d.latest_version).map(d => <option key={d.id} value={d.latest_version!.id}>{d.title}</option>)}</Select></FormField>
      <DocumentSlot projectId={project.id} title="Signed decision" category="decision" canUpload demoMode={Boolean(me?.demo_mode)} />
      <Button loading={busy} disabled={!revision} onClick={() => void run(async () => { const body: CreateDecisionRequest = { kind, title, project_revision_id: revision, basis, legal_reference: legal, valid_from: from || null, valid_to: to || null, permitted_activities: lines(activities), conditions: lines(conditions), restrictions: lines(restrictions), supersedes_id: supersedes || null, change_request_id: null, document_version_id: signed || null }; if (editingId) await apiPatch(`/decisions/${editingId}`, body satisfies PatchDecisionRequest); else await apiPost(`/projects/${project.id}/decisions`, body); clearDraft(); }, 'Decision draft saved')}>Save draft</Button>{editingId && <Button variant="secondary" onClick={clearDraft}>Cancel edit</Button>}
    </CardBody></Card></div>}
    <Dialog open={Boolean(issue)} onClose={() => setIssue(null)} title="Issue decision" footer={<><Button variant="secondary" onClick={() => setIssue(null)}>Keep draft</Button><Button loading={busy} onClick={async () => { if (!issue) return; const id = issue.id; await run(() => apiPost(`/decisions/${id}/issue`), 'Decision issued'); setIssue(null); }}>Issue decision</Button></>}><p>This issues a legally meaningful decision; it cannot be edited afterwards. The research team will see its terms and conditions.</p>{issue?.supersedes_id && <p className="mt-2">It replaces “{permitName(byId.get(issue.supersedes_id) ?? issue)}” only; other permits stay in force.</p>}</Dialog>
  </div>;
}
