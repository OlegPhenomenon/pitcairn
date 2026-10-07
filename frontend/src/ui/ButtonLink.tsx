import { Link, type LinkProps } from 'react-router';
import { cx } from '../lib/cx';
import { sizeClasses, variantClasses, type ButtonSize, type ButtonVariant } from './buttonStyles';

export function ButtonLink({ variant = 'primary', size = 'md', className, ...props }: LinkProps & { variant?: ButtonVariant; size?: ButtonSize }) {
  return <Link className={cx('inline-flex items-center justify-center rounded-md font-medium motion-safe:transition-colors', sizeClasses[size], variantClasses[variant], className)} {...props} />;
}
