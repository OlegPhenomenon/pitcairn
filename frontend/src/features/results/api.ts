import {
  apiPatch,
  apiPost,
  apiPut,
  useApiMutation,
  useApiQuery,
} from "../../api/client";
import type {
  CloseProjectRequest,
  CreateDeliverableRequest,
  CreateSubmissionRequest,
  DeliverableDto,
  ListResponse,
  ProjectWorkspaceDto,
  SetPublicationFilesRequest,
  SubmissionDto,
  UpdateDeliverableRequest,
  UpdatePublicationRequest,
} from "../../api/types";
import { projectKey } from "../projects/api";

export function useResultAction(projectId: string) {
  return useApiMutation<
    unknown,
    { path: string; method?: "POST" | "PATCH" | "PUT"; body?: unknown }
  >(
    ({ path, method = "POST", body }) =>
      method === "PATCH"
        ? apiPatch(path, body)
        : method === "PUT"
          ? apiPut(path, body)
          : apiPost(path, body),
    { invalidate: [projectKey(projectId), ["deliverables", projectId]] },
  );
}

export function useCreateDeliverable(projectId: string) {
  return useApiMutation<DeliverableDto, CreateDeliverableRequest>(
    (body) => apiPost(`/projects/${projectId}/deliverables`, body),
    { invalidate: [projectKey(projectId), ["deliverables", projectId]] },
  );
}

export function useUpdateDeliverable(projectId: string, id: string) {
  return useApiMutation<DeliverableDto, UpdateDeliverableRequest>(
    (body) => apiPatch(`/deliverables/${id}`, body),
    { invalidate: [projectKey(projectId)] },
  );
}

export function useSubmitResult(projectId: string, id: string) {
  return useApiMutation<SubmissionDto, CreateSubmissionRequest>(
    (body) => apiPost(`/deliverables/${id}/submissions`, body),
    { invalidate: [projectKey(projectId), ["deliverables", projectId]] },
  );
}

export function useSubmissionHistory(projectId: string, id: string) {
  return useApiQuery<ListResponse<SubmissionDto>>(
    ["deliverables", projectId, id, "submissions"],
    `/deliverables/${id}/submissions`,
  );
}

export function usePublish(projectId: string, id: string) {
  return useApiMutation<
    unknown,
    { publication: UpdatePublicationRequest; files: SetPublicationFilesRequest }
  >(
    async ({ publication, files }) => {
      await apiPut(`/deliverables/${id}/publication-files`, files);
      return apiPatch(`/deliverables/${id}/publication`, publication);
    },
    { invalidate: [projectKey(projectId)] },
  );
}

export function useCloseProject(projectId: string) {
  return useApiMutation<ProjectWorkspaceDto, CloseProjectRequest>(
    (body) => apiPost(`/projects/${projectId}/close`, body),
    { invalidate: [projectKey(projectId)] },
  );
}
