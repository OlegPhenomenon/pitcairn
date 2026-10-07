import { createContext, useContext } from 'react';

interface FieldCtx {
  id: string;
  describedBy: string | undefined;
  invalid: boolean;
}

export const FieldContext = createContext<FieldCtx>({
  id: '',
  describedBy: undefined,
  invalid: false,
});

export function useFieldIds() {
  return useContext(FieldContext);
}

