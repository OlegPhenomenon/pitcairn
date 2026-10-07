import {
  apiGet,
  apiPatch,
  apiPost,
  useApiMutation,
  useApiQuery,
} from "../../api/client";
import type { ListResponse } from "../../api/types";
import type { ResourceDto } from "../../api/generated/ResourceDto";
import type { TariffDto } from "../../api/generated/TariffDto";
import type { CreateResourceRequest } from "../../api/generated/CreateResourceRequest";
import type { PatchResourceRequest } from "../../api/generated/PatchResourceRequest";
import type { CreateTariffRequest } from "../../api/generated/CreateTariffRequest";
export const resourcesKey = ["resources"] as const;
export const tariffKey = (id: string) => ["resources", id, "tariffs"] as const;
export const useResources = (enabled = true) =>
  useApiQuery<ListResponse<ResourceDto>>(resourcesKey, "/resources", {
    enabled,
  });
export const useTariffs = (id: string) =>
  useApiQuery<ListResponse<TariffDto>>(
    tariffKey(id),
    `/resources/${id}/tariffs`,
    { enabled: !!id },
  );
export const getTariffs = (id: string) =>
  apiGet<ListResponse<TariffDto>>(`/resources/${id}/tariffs`);
export const useCreateResource = () =>
  useApiMutation<ResourceDto, CreateResourceRequest>(
    (body) => apiPost("/resources", body),
    { invalidate: [resourcesKey] },
  );
export const usePatchResource = () =>
  useApiMutation<ResourceDto, { id: string; body: PatchResourceRequest }>(
    ({ id, body }) => apiPatch(`/resources/${id}`, body),
    { invalidate: [resourcesKey] },
  );
export const useCreateTariff = (id: string) =>
  useApiMutation<TariffDto, CreateTariffRequest>(
    (body) => apiPost(`/resources/${id}/tariffs`, body),
    { invalidate: [tariffKey(id)] },
  );
