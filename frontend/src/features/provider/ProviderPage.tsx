import { useState } from "react";
import { fieldError, useApiQuery } from "../../api/client";
import type { ListResponse } from "../../api/types";
import type { ProviderBookingDto } from "../../api/generated/ProviderBookingDto";
import { toNum } from "../../lib/format";
import {
  Banner,
  Button,
  Dialog,
  FormField,
  PageHeader,
  PageLoading,
  StatusBadge,
  Table,
  Textarea,
  useToast,
} from "../../ui";
import { useMe } from "../auth/api";
import { DateRange } from "../resources/display";
import { useConfirmBooking, useDeclineBooking } from "../trips/api";
export function ProviderPage() {
  const me = useMe();
  const allowed = !!me.data?.user.roles.includes("provider");
  const key = ["provider", "bookings"] as const;
  const bookings = useApiQuery<ListResponse<ProviderBookingDto>>(
    key,
    "/provider/bookings",
    { enabled: allowed },
  );
  const confirm = useConfirmBooking([key]);
  const decline = useDeclineBooking([key]);
  const [id, setId] = useState<string | null>(null);
  const [reason, setReason] = useState("");
  const [conflict, setConflict] = useState("");
  const toast = useToast();
  if (!allowed)
    return <Banner tone="error">This page is for service providers.</Banner>;
  return (
    <div className="space-y-5">
      <PageHeader
        title="My booking requests"
        subtitle="Only bookings for services you provide"
      />
      <Banner>
        Providers see project dates, team size, and the lead’s name. Team
        documents are not shared with providers.
      </Banner>
      {conflict && <Banner tone="error">{conflict}</Banner>}
      {bookings.isPending ? (
        <PageLoading />
      ) : bookings.isError ? (
        <Banner tone="error">
          {bookings.error.message}{" "}
          <button className="underline" onClick={() => void bookings.refetch()}>
            Try again
          </button>
        </Banner>
      ) : (
        <Table
          rows={bookings.data?.items ?? []}
          rowKey={(b) => b.booking_id}
          empty={{ title: "No booking requests yet" }}
          columns={[
            {
              header: "Project",
              cell: (b) => (
                <>
                  <strong>{b.project_title}</strong>
                  <span className="block text-slate-500">
                    {b.project_reference ?? "Draft"}
                  </span>
                </>
              ),
            },
            {
              header: "Service and dates",
              cell: (b) => (
                <>
                  {b.resource_name}
                  <span className="block">
                    <DateRange start={b.start_date} end={b.end_date} />
                  </span>
                </>
              ),
            },
            {
              header: "Team",
              cell: (b) => (
                <>
                  {toNum(b.team_size)} people · {b.lead_name}
                </>
              ),
            },
            {
              header: "Status",
              cell: (b) => (
                <>
                  <StatusBadge status={b.status} />
                  {b.decline_reason && (
                    <span className="block text-red-700">
                      {b.decline_reason}
                    </span>
                  )}
                </>
              ),
            },
            {
              header: "Action",
              cell: (b) =>
                b.status === "requested" ? (
                  <span className="flex flex-wrap gap-2">
                    <Button
                      size="sm"
                      loading={confirm.isPending}
                      onClick={async () => {
                        setConflict("");
                        try {
                          await confirm.mutateAsync(b.booking_id);
                          toast.success("Booking confirmed");
                        } catch (e) {
                          if (e instanceof Error) setConflict(e.message);
                        }
                      }}
                    >
                      Confirm
                    </Button>
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => {
                        setId(b.booking_id);
                        setReason("");
                      }}
                    >
                      Decline
                    </Button>
                  </span>
                ) : null,
            },
          ]}
        />
      )}
      <Dialog
        open={!!id}
        onClose={() => setId(null)}
        title="Decline request"
        footer={
          <>
            <Button variant="secondary" onClick={() => setId(null)}>
              Keep request
            </Button>
            <Button
              variant="danger"
              loading={decline.isPending}
              onClick={async () => {
                if (!id) return;
                try {
                  await decline.mutateAsync({ id, body: { reason } });
                  toast.success("Booking declined");
                  setId(null);
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
          error={fieldError(decline.error, "reason")}
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
