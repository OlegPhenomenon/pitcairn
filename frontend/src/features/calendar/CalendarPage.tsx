import { useMemo, useState } from "react";
import { Link } from "react-router";
import { ApiError, fieldError, useApiQuery } from "../../api/client";
import type { CalendarResponse } from "../../api/generated/CalendarResponse";
import type { CalendarBookingDto } from "../../api/generated/CalendarBookingDto";
import { toNum } from "../../lib/format";
import {
  Banner,
  Button,
  DateInput,
  Dialog,
  EmptyState,
  FormField,
  PageHeader,
  PageLoading,
  Select,
  StatusBadge,
  Textarea,
  useToast,
} from "../../ui";
import { useMe } from "../auth/api";
import { useConfirmBooking, useDeclineBooking } from "../trips/api";
import { DateText, dateRange } from "../resources/display";
import { CapacityCell } from "./CapacityCell";

const monthBounds = (month: string) => {
  const [y, m] = month.split("-").map(Number);
  return {
    from: `${month}-01`,
    to: new Date(Date.UTC(y, m, 0)).toISOString().slice(0, 10),
  };
};
export function CalendarPage() {
  const me = useMe();
  const roles = me.data?.user.roles ?? [];
  const allowed =
    roles.includes("base_manager") || roles.includes("coordinator");
  const canDecide = roles.includes("base_manager");
  const [month, setMonth] = useState(new Date().toISOString().slice(0, 7));
  const [kind, setKind] = useState("all");
  const [selected, setSelected] = useState<{
    date: string;
    resource: string;
    bookings: CalendarBookingDto[];
  } | null>(null);
  const [decline, setDecline] = useState<string | null>(null);
  const [reason, setReason] = useState("");
  const [conflict, setConflict] = useState("");
  const toast = useToast();
  const { from, to } = monthBounds(month);
  const key = ["calendar", month] as const;
  const calendar = useApiQuery<CalendarResponse>(
    key,
    `/calendar?from=${from}&to=${to}`,
    { enabled: allowed },
  );
  const confirm = useConfirmBooking([key]);
  const declineMutation = useDeclineBooking([key]);
  const rows = useMemo(
    () =>
      (calendar.data?.resources ?? []).filter(
        (r) => kind === "all" || r.kind === kind,
      ),
    [calendar.data, kind],
  );
  const pending = rows
    .flatMap((r) =>
      r.days.flatMap((d) =>
        d.bookings
          .filter((b) => b.status === "requested")
          .map((b) => ({
            ...b,
            resource: r.name,
            decidable: !r.provider_user_id,
          })),
      ),
    )
    .filter(
      (b, i, a) => a.findIndex((x) => x.booking_id === b.booking_id) === i,
    );
  const decide = async (id: string) => {
    setConflict("");
    try {
      await confirm.mutateAsync(id);
      toast.success("Booking confirmed");
      void calendar.refetch();
    } catch (e) {
      if (e instanceof ApiError && e.code === "capacity_conflict")
        setConflict(e.message);
    }
  };
  if (!allowed)
    return (
      <Banner tone="error">
        This calendar is for base managers and coordinators.
      </Banner>
    );
  return (
    <div className="space-y-5">
      <PageHeader
        title="Base resource calendar"
        subtitle="Confirmed use and requests by day"
      />
      <div className="flex flex-wrap gap-3">
        <FormField label="Month">
          <InputMonth value={month} onChange={setMonth} />
        </FormField>
        <FormField label="Resource kind">
          <Select value={kind} onChange={(e) => setKind(e.target.value)}>
            {["all", "room", "lab", "equipment", "boat", "service"].map((k) => (
              <option key={k} value={k}>
                {k === "all" ? "All kinds" : k}
              </option>
            ))}
          </Select>
        </FormField>
      </div>
      {conflict && <Banner tone="error">{conflict}</Banner>}
      {calendar.isPending ? (
        <PageLoading />
      ) : calendar.isError ? (
        <Banner tone="error">
          {calendar.error.message}{" "}
          <button className="underline" onClick={() => void calendar.refetch()}>
            Try again
          </button>
        </Banner>
      ) : rows.length === 0 ? (
        <EmptyState title="No resources for this filter" />
      ) : (
        <>
          <div className="hidden overflow-x-auto rounded-lg border border-slate-200 bg-white lg:block">
            <table className="w-full border-collapse text-xs">
              <thead>
                <tr>
                  <th className="sticky left-0 z-10 min-w-40 bg-white p-2 text-left">
                    Resource
                  </th>
                  {rows[0].days.map((d) => (
                    <th
                      key={d.date}
                      className="min-w-9 p-1 text-center"
                      title={d.date}
                    >
                      {Number(d.date.slice(-2))}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.resource_id} className="border-t border-slate-100">
                    <th className="sticky left-0 bg-white p-2 text-left font-medium">
                      {r.name}
                      <span className="block text-slate-500">{r.kind}</span>
                    </th>
                    {r.days.map((d) => {
                      return (
                        <td key={d.date} className="p-0.5">
                          <CapacityCell
                            resource={r.name}
                            date={d.date}
                            used={d.used}
                            capacity={r.capacity}
                            unit={r.unit_label}
                            onClick={() =>
                              setSelected({
                                date: d.date,
                                resource: r.name,
                                bookings: d.bookings,
                              })
                            }
                          />
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="space-y-4 lg:hidden">
            {rows[0].days.map((day, i) => (
              <section
                key={day.date}
                className="rounded-lg border border-slate-200 bg-white p-3"
              >
                <h2 className="mb-2 font-semibold">
                  <DateText value={day.date} />
                </h2>
                <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                  {rows.map((r) => {
                    const d = r.days[i];
                    return (
                      <button
                        key={r.resource_id}
                        className="rounded border border-slate-200 p-2 text-left text-sm"
                        onClick={() =>
                          setSelected({
                            date: d.date,
                            resource: r.name,
                            bookings: d.bookings,
                          })
                        }
                      >
                        {r.name} · {toNum(d.used)}/{toNum(r.capacity)}{" "}
                        {r.unit_label}
                      </button>
                    );
                  })}
                </div>
              </section>
            ))}
          </div>
        </>
      )}
      <section>
        <h2 className="mb-3 text-lg font-semibold">Pending requests</h2>
        {pending.length === 0 ? (
          <EmptyState title="No pending requests" />
        ) : (
          <div className="grid gap-3 md:grid-cols-2">
            {pending.map((b) => (
              <div
                key={b.booking_id}
                className="rounded-lg border border-slate-200 bg-white p-4"
              >
                <p className="font-semibold">
                  {b.resource} · {b.project_reference ?? b.project_title}
                </p>
                <p className="text-sm text-slate-600">
                  {dateRange(b.start_date, b.end_date)} · {toNum(b.quantity)}{" "}
                  requested
                </p>
                <StatusBadge status={b.status} />
                {canDecide && b.decidable && (
                  <div className="mt-3 flex gap-2">
                    <Button
                      size="sm"
                      loading={confirm.isPending}
                      onClick={() => void decide(b.booking_id)}
                    >
                      Confirm
                    </Button>
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => {
                        setDecline(b.booking_id);
                        setReason("");
                      }}
                    >
                      Decline
                    </Button>
                  </div>
                )}
                {!b.decidable && (
                  <p className="mt-2 text-xs text-slate-600">
                    The provider confirms this service.
                  </p>
                )}
              </div>
            ))}
          </div>
        )}
      </section>
      <Dialog
        open={!!selected}
        onClose={() => setSelected(null)}
        title={`${selected?.resource} · ${selected?.date}`}
      >
        <p className="mb-3 text-sm">Bookings on this day:</p>
        {selected?.bookings.length ? (
          <ul className="space-y-2">
            {selected.bookings.map((b) => (
              <li key={b.booking_id} className="rounded border p-2 text-sm">
                <Link
                  className="text-teal-700 underline"
                  to={`/app/projects/${b.project_id}/trips`}
                >
                  {b.project_reference ?? b.project_title}
                </Link>{" "}
                · {toNum(b.quantity)} · <StatusBadge status={b.status} />
              </li>
            ))}
          </ul>
        ) : (
          <EmptyState title="No bookings this day" />
        )}
      </Dialog>
      <Dialog
        open={!!decline}
        onClose={() => setDecline(null)}
        title="Decline booking"
        footer={
          <>
            <Button variant="secondary" onClick={() => setDecline(null)}>
              Keep request
            </Button>
            <Button
              loading={declineMutation.isPending}
              variant="danger"
              onClick={async () => {
                if (!decline) return;
                try {
                  await declineMutation.mutateAsync({
                    id: decline,
                    body: { reason },
                  });
                  toast.success("Booking declined");
                  setDecline(null);
                  void calendar.refetch();
                } catch {
                  /* inline */
                }
              }}
            >
              Decline
            </Button>
          </>
        }
      >
        <FormField
          label="Reason"
          required
          error={fieldError(declineMutation.error, "reason")}
        >
          <Textarea
            value={reason}
            onChange={(e) => setReason(e.target.value)}
          />
        </FormField>
      </Dialog>
    </div>
  );
}
function InputMonth({
  value,
  onChange,
}: {
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <DateInput
      type="month"
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}
