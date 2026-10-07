import {
  apiPatch,
  apiPost,
  useApiMutation,
  useApiQuery,
} from "../../api/client";
import type { ListResponse } from "../../api/types";
import type { TripDto } from "../../api/generated/TripDto";
import type { BookingDto } from "../../api/generated/BookingDto";
import type { CreateTripRequest } from "../../api/generated/CreateTripRequest";
import type { PatchTripRequest } from "../../api/generated/PatchTripRequest";
import type { CreateBookingRequest } from "../../api/generated/CreateBookingRequest";
import type { DeclineBookingRequest } from "../../api/generated/DeclineBookingRequest";
export const tripsKey = (id: string) => ["projects", id, "trips"] as const;
export const useTrips = (id: string) =>
  useApiQuery<ListResponse<TripDto>>(tripsKey(id), `/projects/${id}/trips`, {
    enabled: !!id,
  });
export const useCreateTrip = (id: string) =>
  useApiMutation<TripDto, CreateTripRequest>(
    (body) => apiPost(`/projects/${id}/trips`, body),
    { invalidate: [tripsKey(id), ["projects", id]] },
  );
export const usePatchTrip = (id: string) =>
  useApiMutation<TripDto, { tripId: string; body: PatchTripRequest }>(
    ({ tripId, body }) => apiPatch(`/trips/${tripId}`, body),
    { invalidate: [tripsKey(id), ["projects", id]] },
  );
export const useCancelTrip = (id: string) =>
  useApiMutation<TripDto, string>(
    (tripId) => apiPost(`/trips/${tripId}/cancel`),
    { invalidate: [tripsKey(id), ["projects", id]] },
  );
export const useCreateBooking = (id: string) =>
  useApiMutation<BookingDto, { tripId: string; body: CreateBookingRequest }>(
    ({ tripId, body }) => apiPost(`/trips/${tripId}/bookings`, body),
    { invalidate: [tripsKey(id), ["projects", id]] },
  );
export const useCancelBooking = (id: string) =>
  useApiMutation<BookingDto, string>(
    (bookingId) => apiPost(`/bookings/${bookingId}/cancel`),
    { invalidate: [tripsKey(id), ["projects", id]] },
  );
export const useConfirmBooking = (
  invalidate: readonly (readonly unknown[])[],
) =>
  useApiMutation<BookingDto, string>(
    (id) => apiPost(`/bookings/${id}/confirm`),
    { invalidate },
  );
export const useDeclineBooking = (
  invalidate: readonly (readonly unknown[])[],
) =>
  useApiMutation<BookingDto, { id: string; body: DeclineBookingRequest }>(
    ({ id, body }) => apiPost(`/bookings/${id}/decline`, body),
    { invalidate },
  );
