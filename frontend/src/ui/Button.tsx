import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from 'react';
import { Loader2 } from 'lucide-react';

import { cx } from '../lib/cx';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';
export type ButtonSize = 'sm' | 'md';

const variantClasses: Record<ButtonVariant, string> = {
  primary:
    'bg-teal-700 text-white hover:bg-teal-800 active:bg-teal-900 shadow-sm disabled:bg-teal-700/50',
  secondary:
    'bg-white text-slate-800 border border-slate-300 hover:bg-slate-50 active:bg-slate-100 shadow-sm disabled:text-slate-400',
  ghost:
    'text-slate-700 hover:bg-slate-100 active:bg-slate-200 disabled:text-slate-400',
  danger:
    'bg-red-700 text-white hover:bg-red-800 active:bg-red-900 shadow-sm disabled:bg-red-700/50',
};

const sizeClasses: Record<ButtonSize, string> = {
  sm: 'px-2.5 py-1.5 text-sm gap-1.5',
  md: 'px-4 py-2 text-sm gap-2',
};

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  icon?: ReactNode;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  function Button(
    { variant = 'primary', size = 'md', loading, icon, className, children, disabled, ...rest },
    ref,
  ) {
    return (
      <button
        ref={ref}
        className={cx(
          'inline-flex items-center justify-center rounded-md font-medium',
          'motion-safe:transition-colors cursor-pointer',
          'disabled:cursor-not-allowed',
          sizeClasses[size],
          variantClasses[variant],
          className,
        )}
        disabled={disabled || loading}
        aria-busy={loading || undefined}
        {...rest}
      >
        {loading ? (
          <Loader2 className="size-4 motion-safe:animate-spin" aria-hidden />
        ) : (
          icon
        )}
        {children}
      </button>
    );
  },
);
