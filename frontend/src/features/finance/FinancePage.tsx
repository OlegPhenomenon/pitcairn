import { useState } from "react";
import { Link } from "react-router";
import { useQueries } from "@tanstack/react-query";
import { apiGet, fieldError, useApiQuery } from "../../api/client";
import type { ListResponse, ProjectListItemDto } from "../../api/types";
import type { InvoiceDto } from "../../api/generated/InvoiceDto";
import type { TripDto } from "../../api/generated/TripDto";
import type { InvoiceLineInput } from "../../api/generated/InvoiceLineInput";
import { toNum } from "../../lib/format";
import {
  Banner,
  Button,
  Dialog,
  EmptyState,
  FormField,
  Input,
  PageHeader,
  PageLoading,
  Select,
  StatusBadge,
  Table,
  Textarea,
  useToast,
} from "../../ui";
import { useMe } from "../auth/api";
import { DateRange } from "../resources/display";
import { money } from '../resources/displayFormat';
import {
  getInvoices,
  useCancelInvoice,
  useCreateInvoice,
  useIssueInvoice,
  useRecordPayment,
  useRefund,
  useRejectPayment,
  useReplaceLines,
  useVerifyPayment,
} from "./api";

export function FinancePage() {
  const me = useMe();
  const allowed = !!me.data?.user.roles.includes("finance");
  const toast = useToast();
  const projects = useApiQuery<ListResponse<ProjectListItemDto>>(
    ["finance", "projects"],
    "/projects?limit=200",
    { enabled: allowed },
  );
  const financeData = useQueries({
    queries: (projects.data?.items ?? []).map((p) => ({
      queryKey: ["finance", "project", p.id],
      queryFn: async () => ({
        project: p,
        invoices: (await getInvoices(p.id)).items,
        trips: (await apiGet<ListResponse<TripDto>>(`/projects/${p.id}/trips`))
          .items,
      }),
    })),
  });
  const records = financeData.flatMap((q) => (q.data ? [q.data] : []));
  const [projectId, setProjectId] = useState("");
  const [status, setStatus] = useState("all");
  const [selected, setSelected] = useState<InvoiceDto | null>(null);
  const [action, setAction] = useState<
    "issue" | "cancel" | "payment" | "refund" | "reject" | null
  >(null);
  const [paymentId, setPaymentId] = useState("");
  const [reason, setReason] = useState("");
  const [amount, setAmount] = useState("");
  const [reference, setReference] = useState("");
  const [method, setMethod] = useState("manual");
  const [due, setDue] = useState("");
  const [lineDraft, setLineDraft] = useState<InvoiceLineInput[]>([]);
  const [bookingIds, setBookingIds] = useState<string[]>([]);
  const [issueKey, setIssueKey] = useState("");
  const record = records.find((r) => r.project.id === projectId) ?? records[0];
  const currentId = record?.project.id ?? "";
  const create = useCreateInvoice(currentId),
    replace = useReplaceLines(currentId),
    issue = useIssueInvoice(currentId),
    cancel = useCancelInvoice(currentId),
    payment = useRecordPayment(currentId),
    refund = useRefund(currentId),
    verify = useVerifyPayment(currentId),
    reject = useRejectPayment(currentId);
  const booked = new Set(
    records.flatMap((r) =>
      r.invoices
        .filter((i) => i.status !== "cancelled")
        .flatMap((i) =>
          i.lines.map((l) => l.booking_id).filter((id): id is string => !!id),
        ),
    ),
  );
  const eligible =
    record?.trips.flatMap((t) =>
      t.bookings
        .filter((b) => b.status === "confirmed" && !booked.has(b.id))
        .map((b) => ({ ...b, trip: t.title })),
    ) ?? [];
  const allInvoices = records.flatMap((r) =>
    r.invoices.map((i) => ({ ...i, project: r.project })),
  );
  const invoices = allInvoices.filter(
    (i) => status === "all" || i.status === status || i.settlement === status,
  );
  const pending = allInvoices.flatMap((i) =>
    i.payments
      .filter((p) => p.status === "pending_verification")
      .map((p) => ({ ...p, invoice: i })),
  );
  const error =
    create.error ??
    replace.error ??
    issue.error ??
    cancel.error ??
    payment.error ??
    refund.error ??
    reject.error;
  const open = (a: typeof action, i: InvoiceDto) => {
    setSelected(i);
    setAction(a);
    setAmount("");
    setReason("");
    setReference("");
    setDue("");
    setIssueKey(crypto.randomUUID());
  };
  if (!allowed)
    return <Banner tone="error">Finance access is required.</Banner>;
  return (
    <div className="space-y-6">
      <PageHeader
        title="Finance"
        subtitle="Issue invoices and manage payments"
      />
      {projects.isPending || financeData.some((q) => q.isPending) ? (
        <PageLoading />
      ) : projects.isError ? (
        <Banner tone="error">{projects.error.message}</Banner>
      ) : financeData.some((q) => q.isError) ? (
        <Banner tone="error">
          Could not load all project finance records.{" "}
          <button
            className="underline"
            onClick={() => financeData.forEach((q) => void q.refetch())}
          >
            Try again
          </button>
        </Banner>
      ) : (
        <>
          <section className="space-y-3">
            <h2 className="text-lg font-semibold">Invoices to issue</h2>
            <FormField label="Project">
              <Select
                value={record?.project.id ?? ""}
                onChange={(e) => {
                  setProjectId(e.target.value);
                  setBookingIds([]);
                }}
              >
                {records.map((r) => (
                  <option key={r.project.id} value={r.project.id}>
                    {r.project.reference ?? "Draft"} · {r.project.title}
                  </option>
                ))}
              </Select>
            </FormField>
            {eligible.length === 0 ? (
              <EmptyState title="No confirmed unbilled bookings" />
            ) : (
              <div className="rounded-lg border bg-white p-4">
                <p className="mb-3 text-sm text-slate-600">
                  Select bookings to price using the tariff effective at each
                  booking’s start date.
                </p>
                {eligible.map((b) => (
                  <label
                    key={b.id}
                    className="flex items-center gap-2 border-t py-2 text-sm"
                  >
                    <input
                      type="checkbox"
                      checked={bookingIds.includes(b.id)}
                      onChange={(e) =>
                        setBookingIds((v) =>
                          e.target.checked
                            ? [...v, b.id]
                            : v.filter((id) => id !== b.id),
                        )
                      }
                    />
                    {b.resource_name} · {b.trip} ·{" "}
                    <DateRange start={b.start_date} end={b.end_date} />
                  </label>
                ))}
                <Button
                  className="mt-3"
                  loading={create.isPending}
                  disabled={!bookingIds.length}
                  onClick={async () => {
                    try {
                      await create.mutateAsync({ booking_ids: bookingIds });
                      toast.success("Draft invoice created");
                      setBookingIds([]);
                      financeData.forEach((q) => void q.refetch());
                    } catch {
                      /* inline or toast */
                    }
                  }}
                >
                  Create draft invoice
                </Button>
                {fieldError(create.error, "booking_ids") && (
                  <p className="text-red-700">
                    {fieldError(create.error, "booking_ids")}
                  </p>
                )}
              </div>
            )}
          </section>
          <section className="space-y-3">
            <div className="flex flex-wrap items-end justify-between gap-3">
              <h2 className="text-lg font-semibold">Invoices</h2>
              <FormField label="Filter by status">
                <Select
                  value={status}
                  onChange={(e) => setStatus(e.target.value)}
                >
                  {[
                    "all",
                    "draft",
                    "issued",
                    "cancelled",
                    "unpaid",
                    "partially_paid",
                    "paid",
                    "overpaid",
                  ].map((s) => (
                    <option key={s} value={s}>
                      {s.replace("_", " ")}
                    </option>
                  ))}
                </Select>
              </FormField>
            </div>
            <Table
              rows={invoices}
              rowKey={(i) => i.id}
              empty={{ title: "No invoices match" }}
              columns={[
                {
                  header: "Invoice",
                  cell: (i) => (
                    <Link
                      className="text-teal-700 underline"
                      to={`/app/projects/${i.project_id}/invoices`}
                    >
                      {i.number ?? "Draft"} ·{" "}
                      {i.project.reference ?? i.project.title}
                    </Link>
                  ),
                },
                {
                  header: "Total",
                  cell: (i) => money(i.total_cents, i.currency),
                },
                {
                  header: "Status",
                  cell: (i) => (
                    <span className="flex flex-wrap gap-1">
                      <StatusBadge status={i.status} />
                      <StatusBadge status={i.settlement} />
                    </span>
                  ),
                },
                {
                  header: "Actions",
                  cell: (i) => (
                    <span className="flex flex-wrap gap-1">
                      {i.status === "draft" && (
                        <>
                          <Button
                            size="sm"
                            variant="secondary"
                            onClick={() => {
                              setSelected(i);
                              setLineDraft(
                                i.lines.map((l) => ({
                                  id: l.id,
                                  booking_id: l.booking_id,
                                  description: l.description,
                                  quantity: l.quantity,
                                  unit: l.unit,
                                  unit_price_cents: l.unit_price_cents,
                                })),
                              );
                              setAction(null);
                            }}
                          >
                            Edit lines
                          </Button>
                          <Button size="sm" onClick={() => open("issue", i)}>
                            Issue
                          </Button>
                        </>
                      )}
                      {i.status !== "cancelled" && (
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() => open("cancel", i)}
                        >
                          Cancel
                        </Button>
                      )}
                      {i.status === "issued" && (
                        <>
                          <Button
                            size="sm"
                            variant="secondary"
                            onClick={() => open("payment", i)}
                          >
                            Record payment
                          </Button>
                          <Button
                            size="sm"
                            variant="secondary"
                            disabled={toNum(i.net_verified_cents) <= 0}
                            onClick={() => open("refund", i)}
                          >
                            Refund
                          </Button>
                        </>
                      )}
                    </span>
                  ),
                },
              ]}
            />
          </section>
          <section>
            <h2 className="mb-3 text-lg font-semibold">Payments to verify</h2>
            <Table
              rows={pending}
              rowKey={(p) => p.id}
              empty={{ title: "No payments awaiting verification" }}
              columns={[
                { header: "Invoice", cell: (p) => p.invoice.number },
                {
                  header: "Payment",
                  cell: (p) => money(p.amount_cents, p.currency),
                },
                { header: "Reference", cell: (p) => p.external_ref ?? "—" },
                {
                  header: "Action",
                  cell: (p) => (
                    <span className="flex flex-wrap gap-2">
                      <Button
                        size="sm"
                        loading={verify.isPending}
                        onClick={async () => {
                          try {
                            await verify.mutateAsync(p.id);
                            toast.success("Payment verified");
                            financeData.forEach((q) => void q.refetch());
                          } catch {
                            /* toast */
                          }
                        }}
                      >
                        Verify
                      </Button>
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => {
                          setSelected(p.invoice);
                          setPaymentId(p.id);
                          setAction("reject");
                          setReason("");
                        }}
                      >
                        Reject
                      </Button>
                    </span>
                  ),
                },
              ]}
            />
          </section>
        </>
      )}
      <Dialog
        open={!!selected && action === null}
        onClose={() => setSelected(null)}
        title="Edit draft lines"
        footer={
          <>
            <Button variant="secondary" onClick={() => setSelected(null)}>
              Close
            </Button>
            <Button
              loading={replace.isPending}
              onClick={async () => {
                if (!selected) return;
                try {
                  await replace.mutateAsync({
                    invoiceId: selected.id,
                    body: { lines: lineDraft },
                  });
                  toast.success("Invoice lines saved");
                  setSelected(null);
                  financeData.forEach((q) => void q.refetch());
                } catch {
                  /* inline */
                }
              }}
            >
              Save lines
            </Button>
          </>
        }
      >
        <div className="space-y-4">
          {fieldError(replace.error, "booking_id") && (
            <Banner tone="error">
              {fieldError(replace.error, "booking_id")}
            </Banner>
          )}
          {lineDraft.map((l, index) => (
            <div key={l.id ?? index} className="space-y-2 rounded border p-3">
              <FormField
                label="Description"
                error={fieldError(replace.error, `lines.${index}.description`)}
              >
                <Input
                  value={l.description}
                  onChange={(e) =>
                    setLineDraft((v) =>
                      v.map((x, j) =>
                        j === index ? { ...x, description: e.target.value } : x,
                      ),
                    )
                  }
                />
              </FormField>
              <FormField
                label="Quantity"
                error={fieldError(replace.error, `lines.${index}.quantity`)}
              >
                <Input
                  type="number"
                  min="0.01"
                  step="0.01"
                  value={l.quantity}
                  onChange={(e) =>
                    setLineDraft((v) =>
                      v.map((x, j) =>
                        j === index
                          ? { ...x, quantity: Number(e.target.value) }
                          : x,
                      ),
                    )
                  }
                />
              </FormField>
              <FormField
                label="Unit"
                error={fieldError(replace.error, `lines.${index}.unit`)}
              >
                <Input
                  value={l.unit}
                  onChange={(e) =>
                    setLineDraft((v) =>
                      v.map((x, j) =>
                        j === index ? { ...x, unit: e.target.value } : x,
                      ),
                    )
                  }
                />
              </FormField>
              <FormField
                label="Unit price (NZ$)"
                error={fieldError(
                  replace.error,
                  `lines.${index}.unit_price_cents`,
                )}
              >
                <Input
                  type="number"
                  min="0"
                  step="0.01"
                  value={toNum(l.unit_price_cents) / 100}
                  onChange={(e) =>
                    setLineDraft((v) =>
                      v.map((x, j) =>
                        j === index
                          ? {
                              ...x,
                              unit_price_cents: BigInt(
                                Math.round(Number(e.target.value) * 100),
                              ),
                            }
                          : x,
                      ),
                    )
                  }
                />
              </FormField>
              <Button
                size="sm"
                variant="ghost"
                onClick={() =>
                  setLineDraft((v) => v.filter((_, j) => j !== index))
                }
              >
                Remove line
              </Button>
            </div>
          ))}
          <Button
            variant="secondary"
            onClick={() =>
              setLineDraft((v) => [
                ...v,
                {
                  id: null,
                  booking_id: null,
                  description: "",
                  quantity: 1,
                  unit: "item",
                  unit_price_cents: 0n,
                },
              ])
            }
          >
            Add line
          </Button>
        </div>
      </Dialog>
      <Dialog
        open={!!action}
        onClose={() => {
          setAction(null);
          setSelected(null);
        }}
        title={
          action === "issue"
            ? "Issue invoice"
            : action === "cancel"
              ? "Cancel invoice"
              : action === "payment"
                ? "Record manual payment"
                : action === "refund"
                  ? "Record refund"
                  : "Reject payment"
        }
        footer={
          <>
            <Button
              variant="secondary"
              onClick={() => {
                setAction(null);
                setSelected(null);
              }}
            >
              Back
            </Button>
            <Button
              variant={
                action === "cancel" || action === "reject"
                  ? "danger"
                  : "primary"
              }
              loading={
                issue.isPending ||
                cancel.isPending ||
                payment.isPending ||
                refund.isPending ||
                reject.isPending
              }
              onClick={async () => {
                if (!selected || !action) return;
                try {
                  if (action === "issue")
                    await issue.mutateAsync({
                      invoiceId: selected.id,
                      body: { due_date: due || null },
                      key: issueKey,
                    });
                  if (action === "cancel")
                    await cancel.mutateAsync({
                      invoiceId: selected.id,
                      body: { reason },
                    });
                  if (action === "payment")
                    await payment.mutateAsync({
                      invoiceId: selected.id,
                      body: {
                        amount_cents: BigInt(Math.round(Number(amount) * 100)),
                        method,
                        external_ref: reference || null,
                        note: null,
                      },
                    });
                  if (action === "refund")
                    await refund.mutateAsync({
                      invoiceId: selected.id,
                      body: {
                        amount_cents: BigInt(Math.round(Number(amount) * 100)),
                        method: "manual",
                        note: reason || null,
                      },
                    });
                  if (action === "reject")
                    await reject.mutateAsync({
                      paymentId,
                      body: { note: reason || null },
                    });
                  toast.success(
                    action === "issue"
                      ? "Invoice issued"
                      : action === "cancel"
                        ? "Invoice cancelled"
                        : action === "refund"
                          ? "Refund recorded"
                          : action === "payment"
                            ? "Payment recorded"
                            : "Payment rejected",
                  );
                  setAction(null);
                  setSelected(null);
                  financeData.forEach((q) => void q.refetch());
                } catch {
                  /* inline or toast */
                }
              }}
            >
              {action === "issue"
                ? "Issue invoice"
                : action === "cancel"
                  ? "Cancel invoice"
                  : action === "payment"
                    ? "Record payment"
                    : action === "refund"
                      ? "Record refund"
                      : "Reject payment"}
            </Button>
          </>
        }
      >
        {action === "issue" && (
          <>
            <p className="mb-3">
              Issuing makes invoice {selected?.number ?? "draft"} final. Its
              lines and prices can no longer be edited.
            </p>
            <FormField label="Due date" error={fieldError(error, "due_date")}>
              <Input
                type="date"
                value={due}
                onChange={(e) => setDue(e.target.value)}
              />
            </FormField>
          </>
        )}
        {action === "cancel" && (
          <>
            <p className="mb-3">
              Cancellation is final. Verified receipts must be refunded first.
            </p>
            {selected && toNum(selected.net_verified_cents) > 0 && (
              <Banner tone="warning">
                Refund {money(selected.net_verified_cents, selected.currency)}{" "}
                before cancelling.
              </Banner>
            )}
            <FormField
              label="Reason"
              required
              error={fieldError(error, "reason")}
            >
              <Textarea
                value={reason}
                onChange={(e) => setReason(e.target.value)}
              />
            </FormField>
          </>
        )}
        {(action === "payment" || action === "refund") && (
          <>
            <FormField
              label="Amount (NZ$)"
              required
              error={fieldError(error, "amount_cents")}
            >
              <Input
                type="number"
                min="0.01"
                max={
                  action === "refund" && selected
                    ? toNum(selected.net_verified_cents) / 100
                    : undefined
                }
                step="0.01"
                value={amount}
                onChange={(e) => setAmount(e.target.value)}
              />
            </FormField>
            {action === "refund" && selected && (
              <p className="text-sm">
                Available to refund:{" "}
                {money(selected.net_verified_cents, selected.currency)}
              </p>
            )}
            {action === "payment" && (
              <>
                <FormField label="Method">
                  <Select
                    value={method}
                    onChange={(e) => setMethod(e.target.value)}
                  >
                    <option value="manual">Manual</option>
                    <option value="bank_transfer">Bank transfer</option>
                  </Select>
                </FormField>
                <FormField
                  label="External reference"
                  error={fieldError(error, "external_ref")}
                >
                  <Input
                    value={reference}
                    onChange={(e) => setReference(e.target.value)}
                  />
                </FormField>
              </>
            )}
            {action === "refund" && (
              <FormField label="Note">
                <Textarea
                  value={reason}
                  onChange={(e) => setReason(e.target.value)}
                />
              </FormField>
            )}
          </>
        )}
        {action === "reject" && (
          <FormField label="Reason">
            <Textarea
              value={reason}
              onChange={(e) => setReason(e.target.value)}
            />
          </FormField>
        )}
      </Dialog>
    </div>
  );
}
