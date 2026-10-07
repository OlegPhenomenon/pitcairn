import type { ReactNode } from 'react';

import { cx } from '../lib/cx';
import { statusInfo } from './badgeStatus';

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
