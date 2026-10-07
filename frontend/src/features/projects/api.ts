import {
  apiPost,
  listQuery,
  useApiMutation,
  useApiQuery,
} from '../../api/client';
import type {
  CreateDocumentRequest,
  CreateProjectRequest,
  DocumentDto,
  DocumentVersionDto,
  ListResponse,
  NewVersionRequest,
  ProjectDto,
  ProjectWorkspaceDto,
  ProjectListItemDto,
} from '../../api/types';

export const PROJECTS_KEY = ['projects'] as const;
export const projectKey = (id: string) => ['projects', id] as const;
export const projectDocsKey = (id: string) => ['projects', id, 'documents'] as const;

export function useProjects(limit = 50, offset = 0) {
  return useApiQuery<ListResponse<ProjectListItemDto>>(
    [...PROJECTS_KEY, limit, offset],
    `/projects${listQuery(limit, offset)}`,
  );
}

export function useProject(id: string | undefined) {
  return useApiQuery<ProjectWorkspaceDto>(projectKey(id ?? ''), `/projects/${id}`, {
    enabled: !!id,
  });
}

export function useCreateProject() {
  return useApiMutation<ProjectDto, CreateProjectRequest>(
    (body) => apiPost<ProjectDto>('/projects', body),
    { invalidate: [PROJECTS_KEY] },
  );
}

export function useProjectDocuments(projectId: string | undefined) {
  return useApiQuery<ListResponse<DocumentDto>>(
    projectDocsKey(projectId ?? ''),
    `/projects/${projectId}/documents`,
    {
      enabled: !!projectId,
      // While any version is scan-pending, poll so the badge turns green.
      refetchInterval: (query) =>
        query.state.data?.items.some(
          (d) => d.latest_version && d.latest_version.scan_status === 'pending',
        )
          ? 3000
          : false,
    },
  );
}

export function useCreateDocument(projectId: string) {
  return useApiMutation<DocumentDto, CreateDocumentRequest>(
    (body) => apiPost<DocumentDto>(`/projects/${projectId}/documents`, body),
    { invalidate: [projectDocsKey(projectId)] },
  );
}

export function useAddDocumentVersion(projectId: string) {
  return useApiMutation<
    DocumentVersionDto,
    { documentId: string } & NewVersionRequest
  >(
    ({ documentId, ...body }) =>
      apiPost<DocumentVersionDto>(`/documents/${documentId}/versions`, body),
    { invalidate: [projectDocsKey(projectId)] },
  );
}

/** Authenticated download URL for a document version (§5 documents). */
export function documentVersionDownloadUrl(versionId: string): string {
  return `/api/v1/document-versions/${versionId}/download`;
}
