import { expect, it } from "vitest";
import { moneyText } from "./display";
it("formats dashboard invoice amounts as NZ dollars with cents", () => {
  expect(moneyText("Invoice INV-2026-0001 · NZD 3820.5")).toBe(
    "Invoice INV-2026-0001 · NZ$3,820.50",
  );
});
