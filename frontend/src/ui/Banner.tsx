import type { ReactNode } from 'react';
import { AlertTriangle, Info, X, XCircle } from 'lucide-react';

import { cx } from '../lib/cx';

export type BannerTone = 'info' | 'warning' | 'error' | 'demo';

const toneStyles: Record<BannerTone, { wrap: string; icon: ReactNode }> = {
  info: {
    wrap: 'border-blue-300 bg-blue-50 text-blue-900',
    icon: <Info className="size-5 shrink-0 text-blue-700" aria-hidden />,
  },
  warning: {
    wrap: 'border-amber-300 bg-amber-50 text-amber-900',
    icon: <AlertTriangle className="size-5 shrink-0 text-amber-700" aria-hidden />,
  },
  error: {
    wrap: 'border-red-300 bg-red-50 text-red-900',
    icon: <XCircle className="size-5 shrink-0 text-red-700" aria-hidden />,
  },
  demo: {
    wrap: 'border-teal-300 bg-teal-50 text-teal-950',
    icon: <Info className="size-5 shrink-0 text-teal-700" aria-hidden />,
  },
};

export function Banner({
  tone = 'info',
  children,
  onDismiss,
  className,
}: {
  tone?: BannerTone;
  children: ReactNode;
  onDismiss?: () => void;
  className?: string;
}) {
  return (
    <div
      role={tone === 'error' ? 'alert' : 'status'}
      className={cx(
        'flex items-start gap-3 rounded-md border px-4 py-3 text-sm',
        toneStyles[tone].wrap,
        className,
      )}
    >
      {toneStyles[tone].icon}
      <div className="min-w-0 flex-1">{children}</div>
      {onDismiss && (
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Dismiss"
          className="shrink-0 rounded p-0.5 hover:bg-black/5"
        >
          <X className="size-4" aria-hidden />
        </button>
      )}
    </div>
  );
}
