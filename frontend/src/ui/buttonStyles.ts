export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'outlineLight';
export type ButtonSize = 'sm' | 'md';

export const variantClasses: Record<ButtonVariant, string> = {
  primary:
    'bg-teal-700 text-white hover:bg-teal-800 active:bg-teal-900 shadow-sm disabled:bg-teal-700/50',
  secondary:
    'bg-white text-slate-800 border border-slate-300 hover:bg-slate-50 active:bg-slate-100 shadow-sm disabled:text-slate-400',
  ghost:
    'text-slate-700 hover:bg-slate-100 active:bg-slate-200 disabled:text-slate-400',
  danger:
    'bg-red-700 text-white hover:bg-red-800 active:bg-red-900 shadow-sm disabled:bg-red-700/50',
  outlineLight:
    'border border-white bg-transparent text-white hover:bg-white hover:text-navy-900 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-white',
};

export const sizeClasses: Record<ButtonSize, string> = {
  sm: 'px-2.5 py-1.5 text-sm gap-1.5',
  md: 'px-4 py-2 text-sm gap-2',
};
