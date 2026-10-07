import {
  apiPost,
  listQuery,
  useApiMutation,
  useApiQuery,
} from '../../api/client';
import type {
  ListResponse,
  NotificationDto,
  ReadAllResponse,
} from '../../api/types';

const KEY = ['notifications'] as const;

/** Recent notifications; polled so the badge count stays fresh. */
export function useNotifications(limit = 20, enabled = true) {
  return useApiQuery<ListResponse<NotificationDto>>(
    [...KEY, limit],
    `/notifications${listQuery(limit)}`,
    { refetchInterval: 20_000, enabled },
  );
}

export function useMarkRead() {
  return useApiMutation<NotificationDto, string>(
    (id) => apiPost<NotificationDto>(`/notifications/${id}/read`),
    { invalidate: [KEY], silent: true },
  );
}

export function useMarkAllRead() {
  return useApiMutation<ReadAllResponse, void>(
    () => apiPost<ReadAllResponse>('/notifications/read-all'),
    { invalidate: [KEY], silent: true },
  );
}
