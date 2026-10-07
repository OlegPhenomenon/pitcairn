import {
  apiPost,
  apiPatch,
  apiDelete,
  listQuery,
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
/** Samples linked to one deliverable, filtered on the server (max page 200). */
export const useDeliverableSamples = (
  projectId: string,
  deliverableId: string,
) =>
  useApiQuery<ListResponse<SampleDto>>(
    [...samplesKey(projectId), "deliverable", deliverableId],
    `/projects/${projectId}/samples${listQuery(200, 0, { deliverable_id: deliverableId })}`,
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
