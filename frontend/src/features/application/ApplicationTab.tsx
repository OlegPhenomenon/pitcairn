import { useEffect, useMemo, useRef, useState } from 'react';
import { Link, useSearchParams } from 'react-router';
import { useQueryClient } from '@tanstack/react-query';
import { ApiError, apiGet, apiPatch, apiPost, fieldError, useApiQuery } from '../../api/client';
import type { DocumentDto, ListResponse, ProjectWorkspaceDto } from '../../api/types';
import type { PatchProjectRequest } from '../../api/generated/PatchProjectRequest';
import type { PatchProjectResponse } from '../../api/generated/PatchProjectResponse';
import type { SubmitResponse } from '../../api/generated/SubmitResponse';
import type { ThreadDto } from '../../api/generated/ThreadDto';
import { Button, Banner, Card, CardBody, CardHeader, Checkbox, Dialog, FormField, Input, Select, Textarea, useToast } from '../../ui';
import { DocumentSlot } from '../upload/DocumentSlot';
import { useProjectContext } from '../projects/projectContext';
import { projectKey, useProjectDocuments } from '../projects/api';
import { QueryState, SectionLink } from './shared';
import { usePermissions } from './permissions';
import { answerToText, parseSchema, type FormFieldSchema } from './schema';
import type { AssistSuggestionDto } from '../../api/generated/AssistSuggestionDto';
import { FillFromText } from './FillFromText';

export function FieldControl({ field, value, change, disabled }: { field: FormFieldSchema; value: unknown; change: (value: unknown) => void; disabled: boolean }) {
  const text = field.type === 'people' && Array.isArray(value) ? value.join('\n') : answerToText(value);
  if (field.type === 'sites') return <SectionLink to="../sites">Manage sites on the map →</SectionLink>;
  if (field.type === 'checkbox') return <Checkbox checked={value === true} disabled={disabled} onChange={e => change(e.target.checked)} label="Yes" />;
  if (field.type === 'select') return <Select value={text} disabled={disabled} onChange={e => change(e.target.value)}><option value="">Choose…</option>{field.options?.map(o => <option key={o}>{o}</option>)}</Select>;
  if (field.type === 'multiselect') return <div className="grid gap-2 sm:grid-cols-2">{field.options?.map((o, i) => <Checkbox key={o} id={`${field.key}-${i}`} disabled={disabled} checked={Array.isArray(value) && value.includes(o)} onChange={e => change(e.target.checked ? [...(Array.isArray(value) ? value : []), o] : (Array.isArray(value) ? value : []).filter(v => v !== o))} label={o} />)}</div>;
  if (field.type === 'textarea' || field.type === 'people') return <Textarea value={text} disabled={disabled} onChange={e => change(field.type === 'people' ? e.target.value.split('\n').map(v => v.trim()).filter(Boolean) : e.target.value)} rows={field.type === 'people' ? 3 : 5} placeholder={field.type === 'people' ? 'One person per line' : undefined} />;
  if (field.type === 'daterange') {
    const dates = value && typeof value === 'object' ? value as Record<string, unknown> : {};
    return <div className="grid gap-2 sm:grid-cols-2"><Input aria-label={`${field.label} start`} type="date" value={answerToText(dates.start)} disabled={disabled} onChange={e => change({ ...dates, start: e.target.value })} /><Input aria-label={`${field.label} end`} type="date" value={answerToText(dates.end)} disabled={disabled} onChange={e => change({ ...dates, end: e.target.value })} /></div>;
  }
  return <Input type={field.type === 'date' || field.type === 'number' ? field.type : 'text'} value={text} disabled={disabled} onChange={e => change(field.type === 'number' ? (e.target.value === '' ? null : Number(e.target.value)) : e.target.value)} />;
}

export function ApplicationTab() {
  const { project, workspace, me } = useProjectContext();
  const permissions = usePermissions();
  const docs = useProjectDocuments(project.id);
  const threads = useApiQuery<ListResponse<ThreadDto>>(['projects', project.id, 'threads'], `/projects/${project.id}/threads`);
  const schema = useMemo(() => parseSchema(workspace.application.template_schema), [workspace.application.template_schema]);
  const [params] = useSearchParams();
  const backup = useMemo(() => { try { return JSON.parse(localStorage.getItem(`pitcairn-draft-${project.id}`) || 'null') as { answers: Record<string, unknown>; title: string; summary: string } | null; } catch { return null; } }, [project.id]);
  const [answers, setAnswers] = useState<Record<string, unknown>>(backup?.answers ?? project.answers);
  const [title, setTitle] = useState(backup?.title ?? project.title);
  const [summary, setSummary] = useState(backup?.summary ?? project.summary);
  const [status, setStatus] = useState('');
  const [error, setError] = useState<ApiError | null>(null);
  const [stale, setStale] = useState(false);
  const [fictional, setFictional] = useState(false);
  const [busy, setBusy] = useState(false);
  const version = useRef(project.version);
  const dirty = useRef(Boolean(backup));
  const changeCount = useRef(0);
  const saving = useRef(false);
  const submitKey = useRef<string | null>(null);
  const query = useQueryClient();
  const toast = useToast();
  const editable = permissions.canEdit;
  useEffect(() => { if (!params.get('anchor')) return; document.getElementById(params.get('anchor')!)?.scrollIntoView({ block: 'center' }); }, [params]);
  useEffect(() => {
    const first = Object.keys(error?.fields ?? {})[0];
    const target = first ? document.getElementById(first) : null;
    target?.scrollIntoView({ block: 'center' });
    target?.querySelector<HTMLElement>('input, textarea, select, button, a')?.focus();
  }, [error]);
  useEffect(() => {
    if (!editable || !dirty.current || stale || saving.current) return;
    const timer = window.setTimeout(async () => {
      saving.current = true; setStatus('Saving…');
      const countAtStart = changeCount.current;
      const body: PatchProjectRequest = { version: version.current, title, summary, keywords: project.keywords, organisation: project.organisation, answers };
      try {
        const result = await apiPatch<PatchProjectResponse>(`/projects/${project.id}`, body);
        version.current = result.version;
        dirty.current = countAtStart !== changeCount.current;
        setStatus(dirty.current ? 'Saving…' : `Saved ${new Date(result.saved_at).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })}`);
        if (!dirty.current) localStorage.removeItem(`pitcairn-draft-${project.id}`);
        else setAnswers(a => ({ ...a }));
        void query.invalidateQueries({ queryKey: projectKey(project.id) });
      } catch (e) {
        if (e instanceof ApiError && e.code === 'stale_version') { setStale(true); setStatus('Changes need review'); }
        else { setStatus('Offline — will retry'); toast.error('Could not save changes', e instanceof Error ? e.message : undefined); }
      } finally { saving.current = false; }
    }, 800);
    return () => clearTimeout(timer);
  }, [answers, title, summary, editable, project.id, project.keywords, project.organisation, query, stale, toast]);
  useEffect(() => { if (!editable || !dirty.current || stale) return; const onOnline = () => setAnswers(a => ({ ...a })); window.addEventListener('online', onOnline); return () => window.removeEventListener('online', onOnline); }, [editable, stale]);
  function edit(next: Record<string, unknown>, t = title, s = summary) { dirty.current = true; changeCount.current += 1; setAnswers(next); setTitle(t); setSummary(s); localStorage.setItem(`pitcairn-draft-${project.id}`, JSON.stringify({ answers: next, title: t, summary: s })); }
  async function submit() {
    if (busy || !editable || (me?.demo_mode && !fictional)) return;
    setBusy(true); setError(null); submitKey.current ??= crypto.randomUUID();
    try {
      if (dirty.current) {
        const saved = await apiPatch<PatchProjectResponse>(`/projects/${project.id}`, { version: version.current, title, summary, keywords: project.keywords, organisation: project.organisation, answers } satisfies PatchProjectRequest);
        version.current = saved.version; dirty.current = false;
      }
      await apiPost<SubmitResponse>(`/projects/${project.id}/submit`, {}, { headers: { 'Idempotency-Key': submitKey.current } });
      localStorage.removeItem(`pitcairn-draft-${project.id}`); toast.success(project.status === 'changes_requested' ? 'Application resubmitted' : 'Application submitted');
      await query.invalidateQueries({ queryKey: projectKey(project.id) }); submitKey.current = null;
    } catch (e) { if (e instanceof ApiError) { setError(e); if (e.code === 'stale_version') setStale(true); } else toast.error('Could not submit'); }
    finally { setBusy(false); }
  }
  async function upgrade() { setBusy(true); try { await apiPost(`/projects/${project.id}/upgrade-template`); toast.success('Form updated'); await query.invalidateQueries({ queryKey: projectKey(project.id) }); } catch (e) { toast.error('Could not update form', e instanceof Error ? e.message : undefined); } finally { setBusy(false); } }
  return <div className="space-y-5">
    {workspace.application.template_outdated && editable && <Banner tone="warning">A newer form is available. <Button size="sm" loading={busy} onClick={upgrade}>Update to the latest form</Button></Banner>}
    {error && <Banner tone="error"><strong>{error.message}</strong>{error.fields && <ul className="mt-2 list-disc pl-5">{Object.entries(error.fields).map(([key, value]) => <li key={key}><a href={`#${key}`} className="underline">{value}</a></li>)}</ul>}</Banner>}
    <div className="flex justify-end text-sm text-slate-600" role="status">{status || (editable ? 'Ready to save' : 'Read only')}</div>
    <Card><CardHeader title="Application details" /><CardBody className="grid gap-4"><div id="title"><FormField label="Project title" required error={fieldError(error, 'title')}><Input value={title} disabled={!editable} onChange={e => edit(answers, e.target.value)} /></FormField></div><div id="summary"><FormField label="Summary" error={fieldError(error, 'summary')}><Textarea value={summary} disabled={!editable} onChange={e => edit(answers, title, e.target.value)} /></FormField></div></CardBody></Card>
    {editable && <FillFromText templateVersionId={project.template_version_id} fields={schema.sections.flatMap(section => section.fields)} onAccept={(suggestions: AssistSuggestionDto[]) => edit({ ...answers, ...Object.fromEntries(suggestions.map(s => [s.field_key, s.value])) })} />}
    {schema.sections.map(section => <Card key={section.key}><CardHeader title={section.title} /><CardBody className="space-y-5">{section.help && <p className="text-sm text-slate-600">{section.help}</p>}{section.fields.map(field => <div key={field.key} id={`answers.${field.key}`}><FormField label={field.label} help={field.help} required={field.required} error={fieldError(error, `answers.${field.key}`)}><FieldControl field={field} value={answers[field.key]} disabled={!editable} change={v => edit({ ...answers, [field.key]: v })} /></FormField>{threads.data?.items.filter(t => t.anchor_type === 'field' && t.anchor_key === field.key).map(t => <p key={t.id} className="mt-2 border-l-2 border-teal-500 pl-3 text-sm"><SectionLink to={`../messages?thread=${t.id}`}>{t.messages.length} comments · Reply</SectionLink></p>)}{permissions.canComment && <SectionLink to={`../messages?anchor=field:${field.key}`}>Ask / comment</SectionLink>}</div>)}</CardBody></Card>)}
    <Card><CardHeader title="Required documents" /><CardBody className="space-y-3"><QueryState loading={docs.isPending} error={docs.error}>{schema.documents.map(slot => <div key={slot.key} id={`documents.${slot.key}`}><DocumentSlot projectId={project.id} title={slot.label} help={slot.help} slotKey={slot.key} category={slot.category} document={docs.data?.items.find(d => d.slot_key === slot.key)} canUpload={editable} demoMode={Boolean(me?.demo_mode)} />{fieldError(error, `documents.${slot.key}`) && <p className="text-sm text-red-700">{fieldError(error, `documents.${slot.key}`)}</p>}{threads.data?.items.filter(t => t.anchor_type === 'document' && t.anchor_key === slot.key).map(t => <p key={t.id} className="mt-2 border-l-2 border-teal-500 pl-3 text-sm"><SectionLink to={`../messages?thread=${t.id}`}>{t.messages.length} comments · Reply</SectionLink></p>)}{permissions.canComment && <SectionLink to={`../messages?anchor=document:${slot.key}`}>Ask / comment</SectionLink>}</div>)}{docs.data?.items.filter((d: DocumentDto) => !schema.documents.some(s => s.key === d.slot_key)).map(d => <DocumentSlot key={d.id} projectId={project.id} title={d.title} document={d} canUpload={editable} demoMode={Boolean(me?.demo_mode)} />)}</QueryState></CardBody></Card>
    {editable && <div className="space-y-3">{me?.demo_mode && <Checkbox checked={fictional} onChange={e => setFictional(e.target.checked)} label="This is fictional test data" />}<Button loading={busy} disabled={Boolean(me?.demo_mode && !fictional)} onClick={submit}>{project.status === 'changes_requested' ? 'Resubmit application' : 'Submit application'}</Button></div>}
    <div><Link to="../revisions" className="text-teal-700 underline">View revisions</Link></div>
    <Dialog open={stale} onClose={() => setStale(false)} title="Someone else changed this application" footer={<Button onClick={async () => { try { const latest = await apiGet<ProjectWorkspaceDto>(`/projects/${project.id}`); version.current = latest.project.version; query.setQueryData(projectKey(project.id), latest); setStale(false); setAnswers(a => ({ ...a })); toast.info('Latest version loaded. Your browser draft remains in the form; review it before saving.'); } catch (e) { toast.error('Could not reload application', e instanceof Error ? e.message : undefined); } }}>Reload latest version</Button>}><p>Your unsaved changes remain in this browser. Reload the latest version, review the draft shown here, then apply it again.</p></Dialog>
  </div>;
}
