import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { InvoiceDto } from "../../api/generated/InvoiceDto";
import { InvoiceDetail } from "./InvoicesTab";
const invoice: InvoiceDto = {
  id: "invoice-1",
  project_id: "project-1",
  number: "INV-2026-0001",
  status: "issued",
  currency: "NZD",
  total_cents: 66500n,
  net_verified_cents: 0n,
  settlement: "unpaid",
  issued_at: "2026-10-07T00:00:00Z",
  due_date: "2026-11-07",
  cancelled_reason: null,
  created_by: "finance",
  created_at: "2026-10-07T00:00:00Z",
  lines: [
    {
      id: "line-1",
      invoice_id: "invoice-1",
      booking_id: "booking-1",
      description: "Room · seven nights",
      quantity: 7,
      unit: "night",
      unit_price_cents: 9500n,
      amount_cents: 66500n,
    },
  ],
  payments: [],
};
describe("InvoiceDetail", () => {
  it("shows priced lines, settlement, and a test card action only while money is due", () => {
    const onPay = vi.fn();
    const { rerender } = render(
      <InvoiceDetail invoice={invoice} canPay onPay={onPay} />,
    );
    expect(screen.getAllByText("NZ$665.00").length).toBeGreaterThan(0);
    expect(screen.getByText("Unpaid")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Pay with test card" }),
    ).toBeTruthy();
    rerender(
      <InvoiceDetail
        invoice={{ ...invoice, net_verified_cents: 66500n, settlement: "paid" }}
        canPay
        onPay={onPay}
      />,
    );
    expect(
      screen.queryByRole("button", { name: "Pay with test card" }),
    ).toBeNull();
    expect(screen.getByText("Paid")).toBeTruthy();
  });
});
