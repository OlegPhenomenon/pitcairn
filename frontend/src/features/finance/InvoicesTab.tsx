import { useState } from "react";
import { Link } from "react-router";
import type { InvoiceDto } from "../../api/generated/InvoiceDto";
import { toNum } from "../../lib/format";
import {
  Banner,
  Button,
  Card,
  CardBody,
  Dialog,
  EmptyState,
  PageLoading,
  StatusBadge,
  Table,
  useToast,
} from "../../ui";
import { useProjectContext } from "../projects/projectContext";
import { DateText } from "../resources/display";
import { money } from '../resources/displayFormat';
import { useInvoices, usePayTestCard } from "./api";
export function InvoiceDetail({
  invoice,
  canPay,
  onPay,
  paying,
}: {
  invoice: InvoiceDto;
  canPay?: boolean;
  onPay?: (id: string) => void;
  paying?: boolean;
}) {
  return (
    <Card>
      <CardBody>
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div>
            <h3 className="text-lg font-semibold">
              {invoice.number ?? "Draft invoice"}
            </h3>
            <p className="text-sm text-slate-600">
              Issued <DateText value={invoice.issued_at} /> · Due{" "}
              <DateText value={invoice.due_date} />
            </p>
          </div>
          <div className="flex gap-2">
            <StatusBadge status={invoice.status} />
            <StatusBadge status={invoice.settlement} />
          </div>
        </div>
        {invoice.cancelled_reason && (
          <p className="mt-2 text-sm text-red-700">
            Cancelled: {invoice.cancelled_reason}
          </p>
        )}
        <div className="mt-3">
          <Table
            rows={invoice.lines}
            rowKey={(l) => l.id}
            empty={{ title: "No lines" }}
            columns={[
              { header: "Description", cell: (l) => l.description },
              { header: "Quantity", cell: (l) => `${l.quantity} ${l.unit}` },
              {
                header: "Unit price",
                cell: (l) => money(l.unit_price_cents, invoice.currency),
              },
              {
                header: "Amount",
                cell: (l) => money(l.amount_cents, invoice.currency),
              },
            ]}
          />
        </div>
        <p className="mt-3 text-right font-semibold">
          Total {money(invoice.total_cents, invoice.currency)}
        </p>
        <p className="text-right text-sm text-slate-600">
          Received {money(invoice.net_verified_cents, invoice.currency)}
        </p>
        {canPay &&
          invoice.status === "issued" &&
          toNum(invoice.net_verified_cents) < toNum(invoice.total_cents) && (
            <div className="mt-3 text-right">
              <Button loading={paying} onClick={() => onPay?.(invoice.id)}>
                Pay with test card
              </Button>
              <p className="mt-1 text-xs text-slate-600">
                TEST MODE · no money moves
              </p>
            </div>
          )}
        <h4 className="mt-5 font-semibold">Payments and refunds</h4>
        <Table
          rows={invoice.payments}
          rowKey={(p) => p.id}
          empty={{ title: "No payments recorded" }}
          columns={[
            { header: "Date", cell: (p) => <DateText value={p.received_at} /> },
            {
              header: "Type",
              cell: (p) => (p.kind === "refund" ? "Refund" : "Payment"),
            },
            {
              header: "Amount",
              cell: (p) => money(p.amount_cents, p.currency),
            },
            { header: "Method", cell: (p) => p.method.replace("_", " ") },
            {
              header: "Status",
              cell: (p) => <StatusBadge status={p.status} />,
            },
          ]}
        />
      </CardBody>
    </Card>
  );
}
export function InvoicesTab() {
  const { project, me } = useProjectContext();
  const invoices = useInvoices(project.id);
  const pay = usePayTestCard(project.id);
  const [payId, setPayId] = useState<string | null>(null);
  const toast = useToast();
  const canPay =
    ["team_editor", "team_lead"].includes(project.my_access) && !!me?.demo_mode;
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap justify-between gap-3">
        <h2 className="text-xl font-semibold">Invoices</h2>
        {me?.user.roles.includes("finance") && (
          <Link className="text-teal-700 underline" to="/app/finance">
            Open finance workspace
          </Link>
        )}
      </div>
      {invoices.isPending ? (
        <PageLoading />
      ) : invoices.isError ? (
        <Banner tone="error">
          {invoices.error.message}{" "}
          <button className="underline" onClick={() => void invoices.refetch()}>
            Try again
          </button>
        </Banner>
      ) : !invoices.data?.items.length ? (
        <EmptyState
          title="No invoices yet"
          body="Issued invoices and payment history will appear here."
        />
      ) : (
        invoices.data.items.map((i) => (
          <InvoiceDetail
            key={i.id}
            invoice={i}
            canPay={canPay}
            onPay={setPayId}
            paying={pay.isPending}
          />
        ))
      )}
      <Dialog
        open={!!payId}
        onClose={() => setPayId(null)}
        title="Pay with test card"
        footer={
          <>
            <Button variant="secondary" onClick={() => setPayId(null)}>
              Back
            </Button>
            <Button
              loading={pay.isPending}
              onClick={async () => {
                if (!payId) return;
                try {
                  const result = await pay.mutateAsync(payId);
                  toast.success(
                    result.duplicate
                      ? "Payment already recorded"
                      : "Test payment recorded",
                  );
                  setPayId(null);
                } catch {
                  /* toast */
                }
              }}
            >
              Record test payment
            </Button>
          </>
        }
      >
        <p>
          TEST MODE: this records a fictional card payment against the invoice.
          No money moves.
        </p>
      </Dialog>
    </div>
  );
}
