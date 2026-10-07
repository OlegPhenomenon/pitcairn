import {
  createContext,
  forwardRef,
  useContext,
  useId,
  type ForwardedRef,
  type InputHTMLAttributes,
  type ReactNode,
  type SelectHTMLAttributes,
  type TextareaHTMLAttributes,
} from 'react';

import { cx } from '../lib/cx';

/**
 * FormField wires label + help + error to a control via ids:
 * error/help text is referenced with aria-describedby, and invalid state
 * with aria-invalid. Usage:
 *
 *   <FormField label="Email" error={fieldError(err,'email')} required>
 *     <Input name="email" />
 *   </FormField>
 */

interface FieldCtx {
  id: string;
  describedBy: string | undefined;
  invalid: boolean;
}

const FieldContext = createContext<FieldCtx>({
  id: '',
  describedBy: undefined,
  invalid: false,
});

export function useFieldIds() {
  return useContext(FieldContext);
}

export interface FormFieldProps {
  label: ReactNode;
  children: ReactNode;
  help?: ReactNode;
  error?: ReactNode;
  required?: boolean;
  className?: string;
  /** Extra text rendered next to the label, e.g. "(optional)". */
  hint?: ReactNode;
}

export function FormField({
  label,
  children,
  help,
  error,
  required,
  className,
  hint,
}: FormFieldProps) {
  const id = useId();
  const helpId = help ? `${id}-help` : undefined;
  const errorId = error ? `${id}-error` : undefined;
  const describedBy = [helpId, errorId].filter(Boolean).join(' ') || undefined;

  return (
    <FieldContext.Provider value={{ id, describedBy, invalid: Boolean(error) }}>
      <div className={cx('flex flex-col gap-1.5', className)}>
        <label htmlFor={id} className="text-sm font-medium text-slate-800">
          {label}
          {required && (
            <span className="text-red-700" aria-hidden>
              {' '}
              *
            </span>
          )}
          {hint && <span className="ml-1 font-normal text-slate-500">{hint}</span>}
        </label>
        {children}
        {help && (
          <p id={helpId} className="text-sm text-slate-600">
            {help}
          </p>
        )}
        {error && (
          <p id={errorId} className="text-sm text-red-700" role="alert">
            {error}
          </p>
        )}
      </div>
    </FieldContext.Provider>
  );
}

const controlBase =
  'w-full rounded-md border bg-white px-3 py-2 text-base text-slate-900 shadow-sm ' +
  'placeholder:text-slate-400 disabled:cursor-not-allowed disabled:bg-slate-50 ' +
  'disabled:text-slate-500 read-only:bg-slate-50';

function controlClass(invalid: boolean, extra?: string) {
  return cx(
    controlBase,
    invalid
      ? 'border-red-500 focus:border-red-600'
      : 'border-slate-300 focus:border-teal-600',
    extra,
  );
}

export const Input = forwardRef(function Input(
  { className, id, ...rest }: InputHTMLAttributes<HTMLInputElement>,
  ref: ForwardedRef<HTMLInputElement>,
) {
  const field = useFieldIds();
  const describedBy = [field.describedBy, rest['aria-describedby']]
    .filter(Boolean)
    .join(' ') || undefined;
  return (
    <input
      ref={ref}
      id={id ?? field.id}
      aria-describedby={describedBy}
      aria-invalid={field.invalid || undefined}
      className={controlClass(field.invalid, className)}
      {...rest}
    />
  );
});

export const Textarea = forwardRef(function Textarea(
  { className, id, rows = 4, ...rest }: TextareaHTMLAttributes<HTMLTextAreaElement>,
  ref: ForwardedRef<HTMLTextAreaElement>,
) {
  const field = useFieldIds();
  return (
    <textarea
      ref={ref}
      id={id ?? field.id}
      rows={rows}
      aria-describedby={field.describedBy}
      aria-invalid={field.invalid || undefined}
      className={controlClass(field.invalid, className)}
      {...rest}
    />
  );
});

export const Select = forwardRef(function Select(
  { className, id, children, ...rest }: SelectHTMLAttributes<HTMLSelectElement>,
  ref: ForwardedRef<HTMLSelectElement>,
) {
  const field = useFieldIds();
  return (
    <select
      ref={ref}
      id={id ?? field.id}
      aria-describedby={field.describedBy}
      aria-invalid={field.invalid || undefined}
      className={controlClass(field.invalid, className)}
      {...rest}
    >
      {children}
    </select>
  );
});

/** `<input type="date">` styled like Input. */
export const DateInput = forwardRef(function DateInput(
  props: InputHTMLAttributes<HTMLInputElement>,
  ref: ForwardedRef<HTMLInputElement>,
) {
  return <Input ref={ref} type="date" {...props} />;
});

export interface CheckboxProps extends InputHTMLAttributes<HTMLInputElement> {
  /** Text placed next to the box. */
  label?: ReactNode;
}

export const Checkbox = forwardRef(function Checkbox(
  { className, id, label, ...rest }: CheckboxProps,
  ref: ForwardedRef<HTMLInputElement>,
) {
  const field = useFieldIds();
  const fallbackId = useId();
  const boxId = (id ?? field.id) || fallbackId;
  const box = (
    <input
      ref={ref}
      type="checkbox"
      id={boxId}
      aria-describedby={field.describedBy}
      aria-invalid={field.invalid || undefined}
      className={cx(
        'size-4 shrink-0 rounded border-slate-400 text-teal-700 accent-teal-700',
        'disabled:cursor-not-allowed disabled:opacity-50',
        className,
      )}
      {...rest}
    />
  );
  if (label === undefined) return box;
  return (
    <span className="inline-flex items-start gap-2">
      {box}
      <label htmlFor={boxId} className="text-sm text-slate-800">
        {label}
      </label>
    </span>
  );
});
