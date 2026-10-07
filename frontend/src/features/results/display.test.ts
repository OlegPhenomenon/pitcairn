import { describe, expect, it } from "vitest";
import type { DeliverableDto } from "../../api/types";
import { agreementText, isOverdue, linkCheckView } from "./display";
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
describe("external link check display", () => {
  const at = "2026-10-07T14:02:00Z";
  it("shows the four outcomes distinctly with reason and check time", () => {
    const missing = linkCheckView({ check_status: "missing", check_reason: "Not found (HTTP 404)", last_checked_at: at });
    expect(missing.label).toBe("Not found");
    expect(missing.tone).toBe("red");
    expect(missing.detail).toMatch(/^Not found \(HTTP 404\) · last checked \d{1,2} Oct 2026/);
    expect(linkCheckView({ check_status: "unreachable", check_reason: "Connection refused", last_checked_at: at }).label).toBe("Unreachable");
    expect(linkCheckView({ check_status: "available", check_reason: "Reachable (HTTP 200)", last_checked_at: at }).tone).toBe("green");
    expect(linkCheckView({ check_status: "unchecked", check_reason: "", last_checked_at: null })).toEqual({ label: "Not checked yet", tone: "slate", detail: "" });
  });
  it("never presents login-required access as data loss", () => {
    const view = linkCheckView({ check_status: "login_required", check_reason: "Login required (HTTP 401)", last_checked_at: at });
    expect(view.label).toBe("Login required");
    expect(view.tone).not.toBe("red");
    expect(view.detail).toContain("may be agreed closed access");
  });
});
