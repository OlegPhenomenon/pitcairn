import type { ReactNode } from 'react';

import { cx } from '../lib/cx';
import { titleize } from '../lib/format';

export type BadgeTone = 'slate' | 'blue' | 'amber' | 'green' | 'red' | 'teal' | 'navy';

const toneClasses: Record<BadgeTone, string> = {
  slate: 'bg-slate-100 text-slate-700 ring-slate-300',
  blue: 'bg-blue-50 text-blue-800 ring-blue-300',
  amber: 'bg-amber-50 text-amber-800 ring-amber-300',
  green: 'bg-green-50 text-green-800 ring-green-300',
  red: 'bg-red-50 text-red-800 ring-red-300',
  teal: 'bg-teal-50 text-teal-800 ring-teal-300',
  navy: 'bg-navy-50 text-navy-800 ring-navy-200',
};

export function Badge({
  tone = 'slate',
  children,
  className,
}: {
  tone?: BadgeTone;
  children: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={cx(
        'inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium ring-1 ring-inset',
        toneClasses[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}

/**
 * Status → { label, tone } for every status enum of architecture §4.
 * Colour language: amber = waiting/needs action, green = done/positive,
 * red = overdue/refused/failed, slate = draft/neutral/closed, blue = moving.
 */
export const STATUS_MAP: Record<string, { label: string; tone: BadgeTone }> = {
  // projects.status
  draft: { label: 'Draft', tone: 'slate' },
  submitted: { label: 'Submitted', tone: 'blue' },
  in_review: { label: 'In review', tone: 'blue' },
  changes_requested: { label: 'Needs your reply', tone: 'amber' },
  approved: { label: 'Approved', tone: 'green' },
  refused: { label: 'Refused', tone: 'red' },
  closed: { label: 'Closed', tone: 'slate' },
  withdrawn: { label: 'Withdrawn', tone: 'slate' },
  // deliverables.status
  proposed: { label: 'Proposed', tone: 'slate' },
  agreed: { label: 'Agreed', tone: 'teal' },
  // (submitted shared with project) / accepted:
  accepted: { label: 'Accepted', tone: 'green' },
  received: { label: 'Received', tone: 'amber' },
  waived: { label: 'Waived', tone: 'slate' },
  cancelled: { label: 'Cancelled', tone: 'slate' },
  // bookings.status
  requested: { label: 'Requested', tone: 'amber' },
  confirmed: { label: 'Confirmed', tone: 'green' },
  declined: { label: 'Declined', tone: 'red' },
  released: { label: 'Released', tone: 'slate' },
  // invoices.status + derived settlement
  issued: { label: 'Issued', tone: 'blue' },
  unpaid: { label: 'Unpaid', tone: 'amber' },
  partially_paid: { label: 'Partially paid', tone: 'amber' },
  paid: { label: 'Paid', tone: 'green' },
  overpaid: { label: 'Overpaid', tone: 'blue' },
  // payments.status
  pending_verification: { label: 'Pending verification', tone: 'amber' },
  verified: { label: 'Verified', tone: 'green' },
  rejected: { label: 'Rejected', tone: 'red' },
  // trips.status
  planned: { label: 'Planned', tone: 'blue' },
  completed: { label: 'Completed', tone: 'green' },
  // action_items.status
  open: { label: 'Open', tone: 'amber' },
  resolved: { label: 'Resolved', tone: 'green' },
  // review_assignments.status
  invited: { label: 'Invited', tone: 'blue' },
  // template_versions.status
  published: { label: 'Published', tone: 'green' },
  retired: { label: 'Retired', tone: 'slate' },
  // decisions.status
  // (draft / issued above)
  // files.scan_status
  pending: { label: 'Pending', tone: 'amber' },
  clean: { label: 'Clean', tone: 'green' },
  // upload_sessions.status
  finalizing: { label: 'Finalizing', tone: 'blue' },
  complete: { label: 'Complete', tone: 'green' },
  aborted: { label: 'Aborted', tone: 'slate' },
  // jobs.status
  queued: { label: 'Queued', tone: 'slate' },
  running: { label: 'Running', tone: 'blue' },
  done: { label: 'Done', tone: 'green' },
  failed: { label: 'Failed', tone: 'red' },
  dead: { label: 'Dead', tone: 'red' },
  // mail_messages.status
  sent: { label: 'Sent', tone: 'green' },
  // dashboards
  overdue: { label: 'Overdue', tone: 'red' },
  // generic
  active: { label: 'Active', tone: 'green' },
  disabled: { label: 'Disabled', tone: 'red' },
  superseded: { label: 'Superseded', tone: 'slate' },
};

export function statusInfo(status: string): { label: string; tone: BadgeTone } {
  return STATUS_MAP[status] ?? { label: titleize(status), tone: 'slate' };
}

export function StatusBadge({
  status,
  label,
  tone,
  className,
}: {
  status: string;
  label?: ReactNode;
  tone?: BadgeTone;
  className?: string;
}) {
  const info = statusInfo(status);
  return (
    <Badge tone={tone ?? info.tone} className={className}>
      {label ?? info.label}
    </Badge>
  );
}
