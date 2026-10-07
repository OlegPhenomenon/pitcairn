import {
  apiPost,
  apiPatch,
  apiDelete,
  useApiMutation,
  useApiQuery,
} from "../../api/client";
import type {
  CreateSampleRequest,
  ListResponse,
  SampleDto,
  UpdateSampleRequest,
} from "../../api/types";
export const samplesKey = (projectId: string) =>
  ["samples", projectId] as const;
export const useSamples = (projectId: string) =>
  useApiQuery<ListResponse<SampleDto>>(
    samplesKey(projectId),
    `/projects/${projectId}/samples`,
  );
export const useSampleAction = (projectId: string) =>
  useApiMutation<
    unknown,
    {
      kind: "create" | "update" | "delete";
      id?: string;
      body?: CreateSampleRequest | UpdateSampleRequest;
    }
  >(
    ({ kind, id, body }) =>
      kind === "delete"
        ? apiDelete(`/samples/${id}`)
        : kind === "update"
          ? apiPatch(`/samples/${id}`, body)
          : apiPost(`/projects/${projectId}/samples`, body),
    { invalidate: [samplesKey(projectId)] },
  );
