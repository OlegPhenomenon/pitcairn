import { useState } from "react";
import { Link } from "react-router";
import { useQueries } from "@tanstack/react-query";
import { apiGet, fieldError } from "../../api/client";
import type { ListResponse } from "../../api/types";
import type { TariffDto } from "../../api/generated/TariffDto";
import type { TripDto } from "../../api/generated/TripDto";
import { toNum } from "../../lib/format";
import {
  Banner,
  Button,
  Card,
  CardBody,
  DateInput,
  Dialog,
  EmptyState,
  FormField,
  Input,
  PageLoading,
  Select,
  StatusBadge,
  useToast,
} from "../../ui";
import { useProjectContext } from "../projects/ProjectLayout";
import { useResources } from "../resources/api";
import { dateRange, DateText, money } from "../resources/display";
import {
  useCancelBooking,
  useCancelTrip,
  useCreateBooking,
  useCreateTrip,
  usePatchTrip,
  useTrips,
} from "./api";

const kinds = ["room", "lab", "equipment", "boat", "service"];
export function TripsTab() {
  const { project, me } = useProjectContext();
  const id = project.id;
  const canEdit =
    project.my_access === "team_editor" ||
    project.my_access === "team_lead" ||
    !!me?.user.roles.includes("coordinator");
  const canPlan = ["in_review", "changes_requested", "approved"].includes(
    project.status,
  );
  const trips = useTrips(id);
  const resources = useResources(canEdit);
  const tariffs = useQueries({
    queries: (resources.data?.items ?? []).map((r) => ({
      queryKey: ["resources", r.id, "tariffs"],
      queryFn: () =>
        apiGet<ListResponse<TariffDto>>(`/resources/${r.id}/tariffs`),
      enabled: canEdit,
    })),
  });
  const [edit, setEdit] = useState<TripDto | "new" | null>(null);
  const [bookingTrip, setBookingTrip] = useState<TripDto | null>(null);
  const [confirm, setConfirm] = useState<{
    kind: "trip" | "booking";
    id: string;
    name: string;
  } | null>(null);
  const [title, setTitle] = useState("");
  const [arrive, setArrive] = useState("");
  const [depart, setDepart] = useState("");
  const [participants, setParticipants] = useState("");
  const [resourceId, setResourceId] = useState("");
  const [start, setStart] = useState("");
  const [end, setEnd] = useState("");
  const [quantity, setQuantity] = useState("1");
  const toast = useToast();
  const create = useCreateTrip(id);
  const patch = usePatchTrip(id);
  const cancelTrip = useCancelTrip(id);
  const book = useCreateBooking(id);
  const cancelBooking = useCancelBooking(id);
  const tripError = create.error ?? patch.error;
  const bookingError = book.error;
  const openTrip = (trip: TripDto | "new") => {
    setEdit(trip);
    setTitle(trip === "new" ? "" : trip.title);
    setArrive(trip === "new" ? "" : trip.arrive_date);
    setDepart(trip === "new" ? "" : trip.depart_date);
    setParticipants(trip === "new" ? "" : trip.participants.join(", "));
    create.reset();
    patch.reset();
  };
  const openBooking = (trip: TripDto) => {
    setBookingTrip(trip);
    setStart(trip.arrive_date);
    setEnd(trip.depart_date);
    setQuantity("1");
    setResourceId("");
    book.reset();
  };
  const chosen = resources.data?.items.find((r) => r.id === resourceId);
  const chosenTariff =
    chosen &&
    tariffs[
      resources.data!.items.findIndex((r) => r.id === chosen.id)
    ]?.data?.items
      .filter((t) => t.effective_from <= start)
      .sort((a, b) => b.effective_from.localeCompare(a.effective_from))[0];
  const datesLocked =
    edit !== null &&
    edit !== "new" &&
    edit.bookings.some((b) => b.status === "confirmed");
  return (
    <div className="space-y-5">
      <Banner>
        Booking a room, boat, or equipment is not a research permit. See the{" "}
        <Link className="underline" to={`/app/projects/${id}/decisions`}>
          Decisions tab
        </Link>{" "}
        for what was decided.
      </Banner>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-xl font-semibold text-navy-900">
          Trips and bookings
        </h2>
        {canEdit && canPlan && (
          <Button onClick={() => openTrip("new")}>Plan a trip</Button>
        )}
      </div>
      {!canPlan && canEdit && (
        <Banner tone="info">
          Trips can be planned once the project is in review.
        </Banner>
      )}
      {trips.isPending ? (
        <PageLoading />
      ) : trips.isError ? (
        <Banner tone="error">
          {trips.error.message}{" "}
          <button className="underline" onClick={() => void trips.refetch()}>
            Try again
          </button>
        </Banner>
      ) : !trips.data?.items.length ? (
        <EmptyState
          title="No trips planned"
          body="Trip dates and resource bookings will appear here."
        />
      ) : (
        trips.data.items.map((trip) => (
          <Card key={trip.id}>
            <CardBody>
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div>
                  <h3 className="text-lg font-semibold">{trip.title}</h3>
                  <p className="text-sm text-slate-600">
                    {dateRange(trip.arrive_date, trip.depart_date)} ·{" "}
                    {trip.participants.length} participant
                    {trip.participants.length === 1 ? "" : "s"}
                  </p>
                  {trip.participants.length > 0 && (
                    <p className="text-sm text-slate-600">
                      {trip.participants.join(", ")}
                    </p>
                  )}
                </div>
                <StatusBadge status={trip.status} />
              </div>
              {trip.bookings.length ? (
                <ul className="mt-4 divide-y divide-slate-100">
                  {trip.bookings.map((b) => (
                    <li
                      key={b.id}
                      className="flex flex-wrap items-center justify-between gap-2 py-3 text-sm"
                    >
                      <div>
                        <strong>{b.resource_name}</strong> · {toNum(b.quantity)}{" "}
                        · {dateRange(b.start_date, b.end_date)}{" "}
                        <StatusBadge status={b.status} />
                        {b.decline_reason && (
                          <p className="text-red-700">
                            Reason: {b.decline_reason}
                          </p>
                        )}
                      </div>
                      {canEdit &&
                        ["requested", "confirmed"].includes(b.status) && (
                          <Button
                            size="sm"
                            variant="secondary"
                            onClick={() =>
                              setConfirm({
                                kind: "booking",
                                id: b.id,
                                name: b.resource_name,
                              })
                            }
                          >
                            Cancel booking
                          </Button>
                        )}
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="mt-3 text-sm text-slate-500">
                  No booking requests yet.
                </p>
              )}
              {canEdit && ["planned", "confirmed"].includes(trip.status) && (
                <div className="mt-4 flex flex-wrap gap-2">
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => openTrip(trip)}
                  >
                    Edit trip
                  </Button>
                  {canPlan && (
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => openBooking(trip)}
                    >
                      Request booking
                    </Button>
                  )}
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() =>
                      setConfirm({
                        kind: "trip",
                        id: trip.id,
                        name: trip.title,
                      })
                    }
                  >
                    Cancel trip
                  </Button>
                </div>
              )}
            </CardBody>
          </Card>
        ))
      )}
      <Dialog
        open={!!edit}
        onClose={() => setEdit(null)}
        title={edit === "new" ? "Plan a trip" : "Edit trip"}
        footer={
          <>
            <Button variant="secondary" onClick={() => setEdit(null)}>
              Close
            </Button>
            <Button
              form="trip-form"
              type="submit"
              loading={create.isPending || patch.isPending}
            >
              Save trip
            </Button>
          </>
        }
      >
        <form
          id="trip-form"
          className="space-y-4"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!edit) return;
            try {
              const people = participants
                .split(",")
                .map((p) => p.trim())
                .filter(Boolean);
              if (edit === "new")
                await create.mutateAsync({
                  title,
                  arrive_date: arrive,
                  depart_date: depart,
                  participants: people,
                });
              else
                await patch.mutateAsync({
                  tripId: edit.id,
                  body: {
                    title,
                    arrive_date: datesLocked ? null : arrive,
                    depart_date: datesLocked ? null : depart,
                    participants: people,
                  },
                });
              toast.success("Trip saved");
              setEdit(null);
            } catch {
              /* field errors render below */
            }
          }}
        >
          <FormField
            label="Trip title"
            error={fieldError(tripError, "title")}
            required
          >
            <Input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              required
            />
          </FormField>
          <FormField
            label="Arrival date"
            error={fieldError(tripError, "arrive_date")}
            required
          >
            <DateInput
              value={arrive}
              onChange={(e) => setArrive(e.target.value)}
              disabled={datesLocked}
              required
            />
          </FormField>
          <FormField
            label="Departure date"
            error={fieldError(tripError, "depart_date")}
            required
          >
            <DateInput
              value={depart}
              onChange={(e) => setDepart(e.target.value)}
              disabled={datesLocked}
              required
            />
          </FormField>
          {datesLocked && (
            <Banner tone="warning">
              Dates are locked because bookings are confirmed;{" "}
              <Link to={`/app/projects/${id}/changes`} className="underline">
                request a change
              </Link>
              .
            </Banner>
          )}
          <FormField
            label="Participants"
            help="Enter names separated by commas."
            error={fieldError(tripError, "participants")}
          >
            <Input
              value={participants}
              onChange={(e) => setParticipants(e.target.value)}
            />
          </FormField>
        </form>
      </Dialog>
      <Dialog
        open={!!bookingTrip}
        onClose={() => setBookingTrip(null)}
        title="Request a booking"
        footer={
          <>
            <Button variant="secondary" onClick={() => setBookingTrip(null)}>
              Close
            </Button>
            <Button form="booking-form" type="submit" loading={book.isPending}>
              Send request
            </Button>
          </>
        }
      >
        <form
          id="booking-form"
          className="space-y-4"
          onSubmit={async (e) => {
            e.preventDefault();
            if (!bookingTrip) return;
            try {
              await book.mutateAsync({
                tripId: bookingTrip.id,
                body: {
                  resource_id: resourceId,
                  start_date: start,
                  end_date: end,
                  quantity: BigInt(quantity),
                },
              });
              toast.success("Booking requested");
              setBookingTrip(null);
            } catch {
              /* inline errors */
            }
          }}
        >
          <FormField
            label="Resource"
            error={fieldError(bookingError, "resource_id")}
            required
          >
            <Select
              value={resourceId}
              onChange={(e) => setResourceId(e.target.value)}
              required
            >
              <option value="">Choose a resource</option>
              {kinds.map((kind) => (
                <optgroup
                  key={kind}
                  label={kind[0].toUpperCase() + kind.slice(1)}
                >
                  {resources.data?.items
                    .filter((r) => r.kind === kind && r.active)
                    .map((r) => (
                      <option key={r.id} value={r.id}>
                        {r.name} · {toNum(r.quantity)} {r.unit_label}
                      </option>
                    ))}
                </optgroup>
              ))}
            </Select>
          </FormField>
          {resources.isPending && <p>Loading resources…</p>}
          {resources.isError && (
            <Banner tone="error">{resources.error.message}</Banner>
          )}
          {chosen && (
            <p className="text-sm text-slate-600">
              Capacity: {toNum(chosen.quantity)} {chosen.unit_label}. Current
              tariff:{" "}
              {chosenTariff ? (
                <>
                  {money(chosenTariff.price_cents, chosenTariff.currency)}{" "}
                  {chosenTariff.unit.replace("_", " ")}, effective{" "}
                  <DateText value={chosenTariff.effective_from} />
                </>
              ) : (
                "No tariff for this date"
              )}
            </p>
          )}
          <FormField
            label="Start date"
            error={fieldError(bookingError, "start_date")}
            required
          >
            <DateInput
              value={start}
              min={bookingTrip?.arrive_date}
              max={bookingTrip?.depart_date}
              onChange={(e) => setStart(e.target.value)}
              required
            />
          </FormField>
          <FormField
            label="End date (checkout)"
            error={fieldError(bookingError, "end_date")}
            required
          >
            <DateInput
              value={end}
              min={start}
              max={bookingTrip?.depart_date}
              onChange={(e) => setEnd(e.target.value)}
              required
            />
          </FormField>
          <FormField
            label="Quantity"
            error={fieldError(bookingError, "quantity")}
            required
          >
            <Input
              type="number"
              min="1"
              max={chosen ? toNum(chosen.quantity) : undefined}
              value={quantity}
              onChange={(e) => setQuantity(e.target.value)}
              required
            />
          </FormField>
          <p className="text-xs text-slate-600">
            Dates cover each day from start up to, but excluding, end. A manager
            or provider must confirm the request.
          </p>
        </form>
      </Dialog>
      <Dialog
        open={!!confirm}
        onClose={() => setConfirm(null)}
        title={`Cancel ${confirm?.kind ?? ""}?`}
        footer={
          <>
            <Button variant="secondary" onClick={() => setConfirm(null)}>
              Keep it
            </Button>
            <Button
              variant="danger"
              loading={cancelTrip.isPending || cancelBooking.isPending}
              onClick={async () => {
                if (!confirm) return;
                try {
                  if (confirm.kind === "trip")
                    await cancelTrip.mutateAsync(confirm.id);
                  else await cancelBooking.mutateAsync(confirm.id);
                  toast.success(
                    `${confirm.kind === "trip" ? "Trip" : "Booking"} cancelled`,
                  );
                  setConfirm(null);
                } catch {
                  /* toast */
                }
              }}
            >
              Cancel {confirm?.kind}
            </Button>
          </>
        }
      >
        <p>
          {confirm?.kind === "trip"
            ? `Cancelling ${confirm.name} releases all its bookings.`
            : `Cancelling the ${confirm?.name} booking releases that resource.`}{" "}
          This cannot be undone.
        </p>
      </Dialog>
    </div>
  );
}
