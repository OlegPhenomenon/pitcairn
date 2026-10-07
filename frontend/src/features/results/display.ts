import type { DeliverableDto } from "../../api/types";
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
