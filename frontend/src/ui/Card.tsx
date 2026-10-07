import type { HTMLAttributes, ReactNode } from 'react';

import { cx } from '../lib/cx';

export function Card({
  className,
  children,
  ...rest
}: HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cx(
        'rounded-lg border border-slate-200 bg-white shadow-sm',
        className,
      )}
      {...rest}
    >
      {children}
    </div>
  );
}

export function CardHeader({
  title,
  actions,
  children,
  className,
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cx(
        'flex flex-wrap items-start justify-between gap-2 border-b border-slate-200 px-4 py-3 sm:px-5',
        className,
      )}
    >
      <div className="min-w-0">
        {title && <h2 className="text-base font-semibold text-navy-900">{title}</h2>}
        {children}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </div>
  );
}

export function CardBody({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return <div className={cx('px-4 py-4 sm:px-5', className)}>{children}</div>;
}
