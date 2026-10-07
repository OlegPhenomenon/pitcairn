import { useState } from "react";
import { useQueries } from "@tanstack/react-query";
import {
  apiPost,
  fieldError,
  useApiMutation,
  useApiQuery,
} from "../../api/client";
import type { ListResponse, ProjectListItemDto } from "../../api/types";
import type { DemoBankNotifyRequest } from "../../api/generated/DemoBankNotifyRequest";
import type { BankNotificationResponse } from "../../api/generated/BankNotificationResponse";
import {
  Banner,
  Button,
  EmptyState,
  FormField,
  Input,
  PageHeader,
  PageLoading,
  Select,
  StatusBadge,
  useToast,
} from "../../ui";
import { useMe } from "../auth/api";
import { getInvoices } from "../finance/api";
import { money } from '../resources/displayFormat';
export function BankPage() {
  const me = useMe();
  const allowed =
    !!me.data?.demo_mode && !!me.data.user.roles.includes("finance");
  const projects = useApiQuery<ListResponse<ProjectListItemDto>>(
    ["bank", "projects"],
    "/projects?limit=200",
    { enabled: allowed },
  );
  const invoices = useQueries({
    queries: (projects.data?.items ?? []).map((p) => ({
      queryKey: ["bank", p.id],
      queryFn: () => getInvoices(p.id),
    })),
  });
  const issued = invoices.flatMap(
    (q) => q.data?.items.filter((i) => i.status === "issued") ?? [],
  );
  const [invoiceId, setInvoiceId] = useState("");
  const [amount, setAmount] = useState("");
  const [reference, setReference] = useState(() => `DEMO-${Date.now()}`);
  const [sent, setSent] = useState<DemoBankNotifyRequest | null>(null);
  const [result, setResult] = useState<BankNotificationResponse | null>(null);
  const toast = useToast();
  const notify = useApiMutation<
    BankNotificationResponse,
    DemoBankNotifyRequest
  >((body) => apiPost("/demo/bank/notify", body));
  const selected = issued.find((i) => i.id === invoiceId);
  const send = async (body: DemoBankNotifyRequest) => {
    try {
      const r = await notify.mutateAsync(body);
      setSent(body);
      setResult(r);
      toast.success(
        r.duplicate
          ? "Duplicate notification ignored"
          : "Bank notification sent",
      );
    } catch {
      /* inline or toast */
    }
  };
  if (!allowed)
    return (
      <Banner tone="error">
        The bank simulator is available to finance in demo mode.
      </Banner>
    );
  return (
    <div className="space-y-5">
      <PageHeader
        title="Bank simulator"
        subtitle="Demonstrate bank payment notifications"
      />
      <Banner tone="demo">
        This is a demo. Sending a notification creates a payment awaiting
        verification. Sending the same invoice, amount and external reference
        again returns <code>duplicate: true</code> and does not add another
        payment.
      </Banner>
      {projects.isPending || invoices.some((q) => q.isPending) ? (
        <PageLoading />
      ) : projects.isError ? (
        <Banner tone="error">{projects.error.message}</Banner>
      ) : invoices.some((q) => q.isError) ? (
        <Banner tone="error">Could not load invoices.</Banner>
      ) : issued.length === 0 ? (
        <EmptyState
          title="No issued invoices"
          body="Issue an invoice from Finance to try the simulator."
        />
      ) : (
        <form
          className="max-w-xl space-y-4 rounded-lg border border-slate-200 bg-white p-5"
          onSubmit={(e) => {
            e.preventDefault();
            if (!invoiceId) return;
            void send({
              invoice_id: invoiceId,
              amount_cents: BigInt(Math.round(Number(amount) * 100)),
              external_ref: reference,
              note: null,
            });
          }}
        >
          <FormField
            label="Invoice"
            required
            error={fieldError(notify.error, "invoice_id")}
          >
            <Select
              value={invoiceId}
              onChange={(e) => {
                setInvoiceId(e.target.value);
                const i = issued.find((x) => x.id === e.target.value);
                setAmount(
                  i
                    ? String(
                        Math.max(
                          0,
                          Number(i.total_cents - i.net_verified_cents),
                        ) / 100,
                      )
                    : "",
                );
                setSent(null);
                setResult(null);
              }}
              required
            >
              <option value="">Choose an issued invoice</option>
              {issued.map((i) => (
                <option key={i.id} value={i.id}>
                  {i.number} · {money(i.total_cents, i.currency)}
                </option>
              ))}
            </Select>
          </FormField>
          {selected && (
            <p className="text-sm text-slate-600">
              Current settlement: <StatusBadge status={selected.settlement} />
            </p>
          )}
          <FormField
            label="Amount (NZ$)"
            required
            error={fieldError(notify.error, "amount_cents")}
          >
            <Input
              type="number"
              min="0.01"
              step="0.01"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              required
            />
          </FormField>
          <FormField
            label="External reference"
            required
            error={fieldError(notify.error, "external_ref")}
          >
            <Input
              value={reference}
              onChange={(e) => setReference(e.target.value)}
              required
            />
          </FormField>
          <div className="flex flex-wrap gap-2">
            <Button type="submit" loading={notify.isPending}>
              Send bank notification
            </Button>
            {sent && (
              <Button
                type="button"
                variant="secondary"
                loading={notify.isPending}
                onClick={() => void send(sent)}
              >
                Send the same notification again
              </Button>
            )}
          </div>
          {result && (
            <Banner tone="info">
              Result: <code>duplicate: {String(result.duplicate)}</code>
              {result.payment_id && ` · Payment ${result.payment_id}`}. Verify
              it in{" "}
              <a className="underline" href="/app/finance">
                Finance
              </a>
              .
            </Banner>
          )}
        </form>
      )}
    </div>
  );
}
