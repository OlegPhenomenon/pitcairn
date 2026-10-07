import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { apiDelete, apiPatch, apiPost, fieldError, useApiQuery } from '../../api/client';
import type { InviteRequest } from '../../api/generated/InviteRequest';
import type { MemberRoleRequest } from '../../api/generated/MemberRoleRequest';
import type { MembersResponse } from '../../api/generated/MembersResponse';
import type { InvitationDto } from '../../api/generated/InvitationDto';
import { Banner, Button, Card, CardBody, CardHeader, Dialog, FormField, Input, Select, StatusBadge, useToast } from '../../ui';
import { useProjectContext } from '../projects/projectContext';
import { DateText, QueryState } from '../application/shared';
import { usePermissions } from '../application/permissions';

export function TeamTab() {
  const { project } = useProjectContext(); const rights = usePermissions(); const query = useQueryClient(); const toast = useToast();
  const key = ['projects', project.id, 'members'];
  const members = useApiQuery<MembersResponse>(key, `/projects/${project.id}/members`);
  const [email, setEmail] = useState(''); const [role, setRole] = useState('editor'); const [invite, setInvite] = useState<InvitationDto | null>(null);
  const [error, setError] = useState<unknown>(null); const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState<{ id: string; name: string; action: 'lead' | 'remove' } | null>(null);
  async function run(action: () => Promise<unknown>, success: string) { setBusy(true); setError(null); try { await action(); toast.success(success); await query.invalidateQueries({ queryKey: key }); } catch (e) { setError(e); toast.error('Could not update team', e instanceof Error ? e.message : undefined); } finally { setBusy(false); } }
  async function sendInvite() { await run(async () => { const result = await apiPost<InvitationDto>(`/projects/${project.id}/invitations`, { email, role } satisfies InviteRequest); setInvite(result); setEmail(''); }, 'Invitation sent'); }
  return <div className="space-y-5"><Card><CardHeader title="Project team" /><CardBody><QueryState loading={members.isPending} error={members.error} empty={members.data?.members.length === 0}><ul className="divide-y divide-slate-100">{members.data?.members.map(member => <li key={member.user_id} className="flex flex-wrap items-center justify-between gap-3 py-3"><div><strong>{member.name}</strong><p className="text-sm text-slate-600">{member.email}</p><StatusBadge status={member.role} /></div>{rights.lead && member.role !== 'lead' && <div className="flex flex-wrap gap-2"><Select aria-label={`Role for ${member.name}`} value={member.role} disabled={busy} onChange={e => void run(() => apiPatch(`/projects/${project.id}/members/${member.user_id}`, { role: e.target.value } satisfies MemberRoleRequest), 'Role changed')}><option value="editor">Editor</option><option value="viewer">Viewer</option></Select><Button size="sm" variant="secondary" onClick={() => setConfirm({ id: member.user_id, name: member.name, action: 'lead' })}>Make lead</Button><Button size="sm" variant="secondary" onClick={() => setConfirm({ id: member.user_id, name: member.name, action: 'remove' })}>Remove</Button></div>}</li>)}</ul></QueryState></CardBody></Card>
    {rights.lead && <Card><CardHeader title="Invite a colleague" /><CardBody className="space-y-3">{Boolean(error) && <Banner tone="error">{error instanceof Error ? error.message : 'Could not send invitation.'}</Banner>}<FormField label="Email" error={fieldError(error, 'email')}><Input type="email" value={email} onChange={e => setEmail(e.target.value)} /></FormField><FormField label="Role" error={fieldError(error, 'role')}><Select value={role} onChange={e => setRole(e.target.value)}><option value="editor">Editor</option><option value="viewer">Viewer</option></Select></FormField><Button loading={busy} disabled={!email} onClick={sendInvite}>Send invitation</Button>{invite?.accept_url && <p className="text-sm">Invitation pending. Share this acceptance link: <a className="break-all text-teal-700 underline" href={invite.accept_url}>{invite.accept_url}</a></p>}</CardBody></Card>}
    {members.data && members.data.invitations.length > 0 && <Card><CardHeader title="Pending invitations" /><CardBody><ul className="space-y-2">{members.data.invitations.map(i => <li key={i.id} className="text-sm">{i.email} · {i.role} · expires <DateText value={i.expires_at} /></li>)}</ul></CardBody></Card>}
    <Dialog open={Boolean(confirm)} title={confirm?.action === 'lead' ? 'Transfer project lead' : 'Remove team member'} onClose={() => setConfirm(null)} footer={<><Button variant="secondary" onClick={() => setConfirm(null)}>Keep member</Button><Button loading={busy} onClick={async () => { if (!confirm) return; const c = confirm; await run(() => c.action === 'lead' ? apiPost(`/projects/${project.id}/members/${c.id}/make-lead`) : apiDelete(`/projects/${project.id}/members/${c.id}`), c.action === 'lead' ? 'Project lead changed' : 'Member removed'); setConfirm(null); }}>{confirm?.action === 'lead' ? 'Transfer lead' : 'Remove member'}</Button></>}><p>{confirm?.action === 'lead' ? `${confirm?.name} will become the lead and control team membership.` : `${confirm?.name}'s project and file access will be revoked immediately.`}</p></Dialog>
  </div>;
}
