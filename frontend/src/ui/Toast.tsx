import {
  useCallback,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { AlertCircle, CheckCircle2, Info, X } from 'lucide-react';

import { cx } from '../lib/cx';

import { ToastContext, type ToastCtx, type ToastItem, type ToastTone } from './toastContext';

const toneStyles: Record<ToastTone, { icon: ReactNode; bar: string }> = {
  info: { icon: <Info className="size-5 text-blue-700" aria-hidden />, bar: 'border-l-blue-600' },
  success: {
    icon: <CheckCircle2 className="size-5 text-green-700" aria-hidden />,
    bar: 'border-l-green-600',
  },
  error: {
    icon: <AlertCircle className="size-5 text-red-700" aria-hidden />,
    bar: 'border-l-red-600',
  },
};

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const nextId = useRef(1);

  const dismiss = useCallback((id: number) => {
    setItems((list) => list.filter((t) => t.id !== id));
  }, []);

  const push = useCallback(
    (t: Omit<ToastItem, 'id'>) => {
      const id = nextId.current++;
      setItems((list) => [...list.slice(-4), { ...t, id }]);
      window.setTimeout(() => dismiss(id), t.tone === 'error' ? 9000 : 6000);
    },
    [dismiss],
  );

  const ctx = useMemo<ToastCtx>(
    () => ({
      push,
      success: (title, body) => push({ tone: 'success', title, body }),
      error: (title, body) => push({ tone: 'error', title, body }),
      info: (title, body) => push({ tone: 'info', title, body }),
    }),
    [push],
  );

  return (
    <ToastContext.Provider value={ctx}>
      {children}
      <div
        aria-live="polite"
        className="pointer-events-none fixed inset-x-3 bottom-3 z-50 flex flex-col gap-2 sm:inset-x-auto sm:right-4 sm:bottom-4 sm:w-96"
      >
        {items.map((t) => (
          <div
            key={t.id}
            role={t.tone === 'error' ? 'alert' : 'status'}
            className={cx(
              'pointer-events-auto flex items-start gap-3 rounded-md border border-l-4 border-slate-200 bg-white p-3 shadow-lg',
              toneStyles[t.tone].bar,
            )}
          >
            <span className="mt-0.5 shrink-0">{toneStyles[t.tone].icon}</span>
            <div className="min-w-0 flex-1">
              <p className="text-sm font-semibold text-slate-900">{t.title}</p>
              {t.body && <p className="mt-0.5 text-sm break-words text-slate-600">{t.body}</p>}
            </div>
            <button
              type="button"
              onClick={() => dismiss(t.id)}
              aria-label="Dismiss notification"
              className="shrink-0 rounded p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-600"
            >
              <X className="size-4" aria-hidden />
            </button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}
