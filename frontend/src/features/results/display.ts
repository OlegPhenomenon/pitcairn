import type { DeliverableDto, ExternalLinkDto } from "../../api/types";
import { formatDateTime } from "../../lib/format";
import type { BadgeTone } from "../../ui";
export const isOverdue = (
  d: DeliverableDto,
  today = new Date().toISOString().slice(0, 10),
) =>
  d.due_date < today && !["accepted", "waived", "cancelled"].includes(d.status);
export const agreementText = (d: DeliverableDto) =>
  d.agreement_state === "agreed"
    ? "Both sides agreed"
    : d.agreement_state === "team_only"
      ? "Waiting for Pitcairn to agree"
      : d.agreement_state === "staff_only"
        ? "Waiting for the team to agree"
        : "Waiting for both sides";

/**
 * How an external link's last availability check is shown. `missing` and
 * `unreachable` mean the data may be lost; `login_required` is NOT loss —
 * closed access may have been agreed.
 */
export function linkCheckView(
  link: Pick<ExternalLinkDto, "check_status" | "check_reason" | "last_checked_at">,
): { label: string; tone: BadgeTone; detail: string } {
  const checked = link.last_checked_at
    ? `last checked ${formatDateTime(link.last_checked_at)}`
    : "";
  const detail = (reason: string) =>
    [reason, checked].filter(Boolean).join(" · ");
  switch (link.check_status) {
    case "available":
      return { label: "Available", tone: "green", detail: detail(link.check_reason) };
    case "missing":
      return { label: "Not found", tone: "red", detail: detail(link.check_reason) };
    case "unreachable":
      return { label: "Unreachable", tone: "red", detail: detail(link.check_reason) };
    case "login_required":
      return {
        label: "Login required",
        tone: "blue",
        detail: detail(`${link.check_reason} — may be agreed closed access`),
      };
    default:
      return { label: "Not checked yet", tone: "slate", detail: "" };
  }
}
