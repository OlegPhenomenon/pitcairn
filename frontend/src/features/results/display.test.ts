import { describe, expect, it } from "vitest";
import type { DeliverableDto } from "../../api/types";
import { agreementText, isOverdue } from "./display";
const sample = {
  due_date: "2026-01-10",
  status: "agreed",
  agreement_state: "team_only",
} as DeliverableDto;
describe("deliverable display", () => {
  it("shows which side must still agree after terms change", () => {
    expect(agreementText(sample)).toBe("Waiting for Pitcairn to agree");
    expect(agreementText({ ...sample, agreement_state: "staff_only" })).toBe(
      "Waiting for the team to agree",
    );
    expect(agreementText({ ...sample, agreement_state: "agreed" })).toBe(
      "Both sides agreed",
    );
  });
  it("only highlights unresolved past due items", () => {
    expect(isOverdue(sample, "2026-01-11")).toBe(true);
    expect(isOverdue({ ...sample, status: "accepted" }, "2026-01-11")).toBe(
      false,
    );
    expect(isOverdue({ ...sample, status: "waived" }, "2026-01-11")).toBe(
      false,
    );
    expect(isOverdue(sample, "2026-01-10")).toBe(false);
  });
});
