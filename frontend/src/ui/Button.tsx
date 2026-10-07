import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from 'react';
import { Loader2 } from 'lucide-react';

import { cx } from '../lib/cx';

import { sizeClasses, variantClasses, type ButtonSize, type ButtonVariant } from './buttonStyles';

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
