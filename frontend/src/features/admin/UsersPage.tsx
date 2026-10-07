import { useState, type FormEvent } from 'react';
import { Plus, UserPlus } from 'lucide-react';

import { ApiError, fieldError } from '../../api/client';
import type { UserDto } from '../../api/types';
import {
  Badge,
  Banner,
  Button,
  Card,
  CardBody,
  CardHeader,
  Checkbox,
  Dialog,
  FormField,
  Input,
  PageHeader,
  Select,
  Table,
  useToast,
} from '../../ui';
import { titleize } from '../../lib/format';
import {
  useAdminUsers,
  useCreateUser,
  useGrantRole,
  usePatchUser,
  useRevokeRole,
} from './api';

const ALL_ROLES = [
  'coordinator',
  'expert',
  'decision_maker',
  'base_manager',
  'finance',
  'admin',
  'provider',
];

function errorText(err: unknown): string {
  if (err instanceof ApiError) {
    if (err.code === 'protected_user')
      return 'That user holds the decision_maker role and is protected — only the server operator (CLI) can change them.';
    if (err.code === 'cannot_grant_decision_maker')
      return 'Only an existing decision maker can grant decision_maker.';
    if (err.code === 'protected_role')
      return 'Only a decision maker can revoke decision_maker.';
    return err.message;
  }
  return 'Something went wrong.';
}

export function AdminUsersPage() {
  const [q, setQ] = useState('');
  const [search, setSearch] = useState('');
  const users = useAdminUsers(search);
  const revoke = useRevokeRole();
  const patch = usePatchUser();
  const toast = useToast();
  const [createOpen, setCreateOpen] = useState(false);
  const [editUser, setEditUser] = useState<UserDto | null>(null);
  const [rolesFor, setRolesFor] = useState<UserDto | null>(null);

  const onSearch = (e: FormEvent) => {
    e.preventDefault();
    setSearch(q.trim());
  };

  return (
    <>
      <PageHeader
        title="Users"
        subtitle="Accounts, roles and access."
        actions={
          <Button icon={<UserPlus className="size-4" />} onClick={() => setCreateOpen(true)}>
            New user
          </Button>
        }
      />
      <Card>
        <CardHeader>
          <form onSubmit={onSearch} className="flex w-full gap-2" role="search">
            <label htmlFor="user-search" className="sr-only">
              Search users by name or email
            </label>
            <Input
              id="user-search"
              placeholder="Search by name or email"
              value={q}
              onChange={(e) => setQ(e.target.value)}
              className="max-w-xs"
            />
            <Button type="submit" variant="secondary" size="sm">
              Search
            </Button>
          </form>
        </CardHeader>
        <CardBody>
          <Table
            loading={users.isPending}
            rows={users.data?.items ?? []}
            rowKey={(u) => u.id}
            caption="Users"
            empty={{ title: 'No users found' }}
            columns={[
              {
                header: 'Name',
                cell: (u) => (
                  <span>
                    <span className="block font-medium text-slate-900">{u.name}</span>
                    <span className="text-xs text-slate-500">{u.email}</span>
                  </span>
                ),
              },
              {
                header: 'Organisation',
                cell: (u) => u.organisation || '—',
                hideOnCard: true,
              },
              {
                header: 'Roles',
                cell: (u) => (
                  <span className="flex flex-wrap gap-1">
                    {u.roles.length === 0 && <span className="text-slate-400">—</span>}
                    {u.roles.map((r) => (
                      <Badge key={r} tone="teal">
                        {titleize(r)}
                      </Badge>
                    ))}
                  </span>
                ),
              },
              {
                header: 'Status',
                cell: (u) => (
                  <Badge tone={u.disabled ? 'red' : 'green'}>
                    {u.disabled ? 'Disabled' : 'Active'}
                  </Badge>
                ),
              },
              {
                header: 'Actions',
                cell: (u) => (
                  <span className="flex flex-wrap justify-end gap-1.5">
                    <Button size="sm" variant="ghost" onClick={() => setRolesFor(u)}>
                      Roles
                    </Button>
                    <Button size="sm" variant="ghost" onClick={() => setEditUser(u)}>
                      Edit
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      loading={patch.isPending && patch.variables?.id === u.id}
                      onClick={async () => {
                        try {
                          await patch.mutateAsync({ id: u.id, name: null, organisation: null, email: null, disabled: !u.disabled });
                          toast.success(u.disabled ? 'User re-enabled' : 'User disabled');
                        } catch (e) {
                          toast.error('Could not update user', errorText(e));
                        }
                      }}
                    >
                      {u.disabled ? 'Enable' : 'Disable'}
                    </Button>
                  </span>
                ),
              },
            ]}
          />
        </CardBody>
      </Card>

      <CreateUserDialog open={createOpen} onClose={() => setCreateOpen(false)} />
      <EditUserDialog user={editUser} onClose={() => setEditUser(null)} />
      <RolesDialog user={rolesFor} onClose={() => setRolesFor(null)} />
      {/* revoke errors are toasted via hook; surface revoke failures too */}
      {revoke.error && (
        <Banner tone="error" className="mt-3" onDismiss={() => revoke.reset()}>
          {errorText(revoke.error)}
        </Banner>
      )}
    </>
  );
}

function CreateUserDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const create = useCreateUser();
  const toast = useToast();
  const [email, setEmail] = useState('');
  const [name, setName] = useState('');
  const [organisation, setOrganisation] = useState('');
  const [password, setPassword] = useState('');

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await create.mutateAsync({ email, name, organisation, password });
      toast.success('User created');
      onClose();
      setEmail('');
      setName('');
      setOrganisation('');
      setPassword('');
    } catch {
      // The mutation error appears in the dialog.
    }
  };

  const err = create.error;

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="New user"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button form="create-user-form" type="submit" loading={create.isPending}>
            Create
          </Button>
        </>
      }
    >
      <form id="create-user-form" onSubmit={submit} className="flex flex-col gap-4" noValidate>
        {err && !err.fields && <Banner tone="error">{errorText(err)}</Banner>}
        <FormField label="Email" required error={fieldError(err, 'email')}>
          <Input type="email" value={email} onChange={(e) => setEmail(e.target.value)} required />
        </FormField>
        <FormField label="Name" required error={fieldError(err, 'name')}>
          <Input value={name} onChange={(e) => setName(e.target.value)} required />
        </FormField>
        <FormField label="Organisation" error={fieldError(err, 'organisation')}>
          <Input value={organisation} onChange={(e) => setOrganisation(e.target.value)} />
        </FormField>
        <FormField
          label="Temporary password"
          required
          error={fieldError(err, 'password')}
          help="At least 8 characters; the user can be asked to change it later."
        >
          <Input
            type="password"
            autoComplete="new-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
            minLength={8}
          />
        </FormField>
      </form>
    </Dialog>
  );
}

function EditUserDialog({ user, onClose }: { user: UserDto | null; onClose: () => void }) {
  const patch = usePatchUser();
  const toast = useToast();
  const [email, setEmail] = useState('');
  const [name, setName] = useState('');
  const [organisation, setOrganisation] = useState('');
  const [err, setErr] = useState<ApiError | null>(null);

  // reset form fields when a different user is opened
  const [prevId, setPrevId] = useState<string | null>(null);
  if (user && user.id !== prevId) {
    setPrevId(user.id);
    setEmail(user.email);
    setName(user.name);
    setOrganisation(user.organisation);
    setErr(null);
  }

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!user) return;
    try {
      await patch.mutateAsync({ id: user.id, email, name, organisation, disabled: null });
      toast.success('User updated');
      onClose();
    } catch (e2) {
      setErr(e2 instanceof ApiError ? e2 : null);
    }
  };

  return (
    <Dialog open={!!user} onClose={onClose} title={`Edit ${user?.name ?? 'user'}`}>
      <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
        {err && <Banner tone="error">{errorText(err)}</Banner>}
        <FormField label="Email" required error={fieldError(err, 'email')}>
          <Input type="email" value={email} onChange={(e) => setEmail(e.target.value)} required />
        </FormField>
        <FormField label="Name" required error={fieldError(err, 'name')}>
          <Input value={name} onChange={(e) => setName(e.target.value)} required />
        </FormField>
        <FormField label="Organisation">
          <Input value={organisation} onChange={(e) => setOrganisation(e.target.value)} />
        </FormField>
        <div className="flex justify-end gap-2">
          <Button variant="ghost" type="button" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" loading={patch.isPending}>
            Save
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function RolesDialog({ user, onClose }: { user: UserDto | null; onClose: () => void }) {
  const grant = useGrantRole();
  const revoke = useRevokeRole();
  const [role, setRole] = useState(ALL_ROLES[0]);
  const [err, setErr] = useState<string | null>(null);
  const [prevId, setPrevId] = useState<string | null>(null);
  if (user && user.id !== prevId) {
    setPrevId(user.id);
    setErr(null);
  }

  const busy = grant.isPending || revoke.isPending;

  return (
    <Dialog
      open={!!user}
      onClose={onClose}
      title={`Roles — ${user?.name ?? ''}`}
    >
      <div className="flex flex-col gap-4">
        {err && (
          <Banner tone="error" onDismiss={() => setErr(null)}>
            {err}
          </Banner>
        )}
        <div>
          <p className="mb-1 text-sm font-medium text-slate-700">Current roles</p>
          {user && user.roles.length > 0 ? (
            <ul className="flex flex-col gap-1">
              {user.roles.map((r) => (
                <li
                  key={r}
                  className="flex items-center justify-between rounded-md border border-slate-200 px-3 py-1.5 text-sm"
                >
                  <span>{titleize(r)}</span>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy}
                    onClick={async () => {
                      try {
                        setErr(null);
                        await revoke.mutateAsync({ userId: user.id, role: r });
                      } catch (e) {
                        setErr(errorText(e));
                      }
                    }}
                  >
                    Remove
                  </Button>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-sm text-slate-500">No roles — this is a plain researcher account.</p>
          )}
        </div>
        <form
          className="flex items-end gap-2"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!user) return;
            try {
              setErr(null);
              await grant.mutateAsync({ userId: user.id, role });
            } catch (e2) {
              setErr(errorText(e2));
            }
          }}
        >
          <FormField label="Add role" className="flex-1">
            <Select value={role} onChange={(e) => setRole(e.target.value)}>
              {ALL_ROLES.map((r) => (
                <option key={r} value={r}>
                  {titleize(r)}
                </option>
              ))}
            </Select>
          </FormField>
          <Button type="submit" size="md" loading={grant.isPending} icon={<Plus className="size-4" />}>
            Add
          </Button>
        </form>
        <p className="text-xs text-slate-500">
          Note: only an existing decision maker can grant or revoke the decision_maker role;
          for everyone else the action returns <code>cannot_grant_decision_maker</code>.
        </p>
      </div>
    </Dialog>
  );
}

// re-export for routes
export { AdminUsersPage as default };

// silence unused import warning for Checkbox (kept for parity)
void Checkbox;
