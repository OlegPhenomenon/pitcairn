export interface FormFieldSchema { key: string; label: string; type: string; required?: boolean; help?: string; options?: string[] }
export interface FormSectionSchema { key: string; title: string; help?: string; fields: FormFieldSchema[] }
export interface DocumentSchema { key: string; label: string; help?: string; category: string }

const object = (value: unknown): Record<string, unknown> => value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
const str = (value: unknown) => typeof value === 'string' ? value : '';
const arr = (value: unknown): unknown[] => Array.isArray(value) ? value : [];

export function parseSchema(value: Record<string, unknown>): { sections: FormSectionSchema[]; documents: DocumentSchema[] } {
  return {
    sections: arr(value.sections).map((raw) => {
      const s = object(raw);
      return { key: str(s.key), title: str(s.title), help: str(s.help), fields: arr(s.fields).map((item) => {
        const f = object(item);
        return { key: str(f.key), label: str(f.label), type: str(f.type), required: f.required === true, help: str(f.help), options: arr(f.options).map(str) };
      }) };
    }),
    documents: arr(value.required_documents).map((raw) => {
      const d = object(raw);
      return { key: str(d.key), label: str(d.label), help: str(d.help), category: str(d.category) || 'application' };
    }),
  };
}

export function answerToText(value: unknown): string {
  if (Array.isArray(value)) return value.map(answerToText).join(', ');
  if (value && typeof value === 'object') return JSON.stringify(value);
  return value == null ? '' : String(value);
}
