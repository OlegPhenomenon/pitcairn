import {
  apiDelete,
  apiPatch,
  apiPost,
  apiPut,
  listQuery,
  useApiMutation,
  useApiQuery,
} from '../../api/client';
import type {
  AdminCreateUserRequest,
  AdminPatchUserRequest,
  AuditEventDto,
  GrantRoleRequest,
  JobDto,
  ListResponse,
  SettingsDto,
  UserDto,
} from '../../api/types';

export const ADMIN_USERS_KEY = ['admin', 'users'] as const;
export const ADMIN_SETTINGS_KEY = ['admin', 'settings'] as const;
export const ADMIN_JOBS_KEY = ['admin', 'jobs'] as const;
export const ADMIN_AUDIT_KEY = ['admin', 'audit'] as const;

export function useAdminUsers(q: string, limit = 50, offset = 0) {
  return useApiQuery<ListResponse<UserDto>>(
    [...ADMIN_USERS_KEY, q, limit, offset],
    `/admin/users${listQuery(limit, offset, { q: q || undefined })}`,
  );
}

export function useCreateUser() {
  return useApiMutation<UserDto, AdminCreateUserRequest>(
    (body) => apiPost<UserDto>('/admin/users', body),
    { invalidate: [ADMIN_USERS_KEY] },
  );
}

export function usePatchUser() {
  return useApiMutation<UserDto, { id: string } & AdminPatchUserRequest>(
    ({ id, ...body }) => apiPatch<UserDto>(`/admin/users/${id}`, body),
    { invalidate: [ADMIN_USERS_KEY] },
  );
}

export function useGrantRole() {
  return useApiMutation<UserDto, { userId: string } & GrantRoleRequest>(
    ({ userId, ...body }) => apiPost<UserDto>(`/admin/users/${userId}/roles`, body),
    { invalidate: [ADMIN_USERS_KEY] },
  );
}

export function useRevokeRole() {
  return useApiMutation<UserDto, { userId: string; role: string }>(
    ({ userId, role }) => apiDelete<UserDto>(`/admin/users/${userId}/roles/${role}`),
    { invalidate: [ADMIN_USERS_KEY] },
  );
}

export function useAdminSettings() {
  return useApiQuery<SettingsDto>(ADMIN_SETTINGS_KEY, '/admin/settings');
}

export function usePutSettings() {
  return useApiMutation<SettingsDto, SettingsDto>(
    (body) => apiPut<SettingsDto>('/admin/settings', body),
    { invalidate: [ADMIN_SETTINGS_KEY] },
  );
}

export function useAdminJobs(status: string | undefined, limit = 50, offset = 0) {
  return useApiQuery<ListResponse<JobDto>>(
    [...ADMIN_JOBS_KEY, status, limit, offset],
    `/admin/jobs${listQuery(limit, offset, { status })}`,
    { refetchInterval: 10_000 },
  );
}

export function useRetryJob() {
  return useApiMutation<JobDto, string>(
    (id) => apiPost<JobDto>(`/admin/jobs/${id}/retry`),
    { invalidate: [ADMIN_JOBS_KEY] },
  );
}

export interface AuditFilter {
  [key: string]: string | number | undefined;
  project_id?: string;
  entity_type?: string;
  entity_id?: string;
}

export function useAdminAudit(filter: AuditFilter, limit = 50, offset = 0) {
  return useApiQuery<ListResponse<AuditEventDto>>(
    [...ADMIN_AUDIT_KEY, filter, limit, offset],
    `/admin/audit${listQuery(limit, offset, filter)}`,
  );
}
