import { describe, expect, it } from 'vitest';
import { answerToText, parseSchema } from './schema';

describe('application schema rendering', () => {
  it('keeps every supported field and required document slot', () => {
    const kinds = ['text', 'textarea', 'date', 'daterange', 'number', 'select', 'multiselect', 'people', 'checkbox', 'sites'];
    const parsed = parseSchema({ sections: [{ key: 'fieldwork', title: 'Fieldwork', fields: kinds.map(type => ({ key: type, label: type, type, required: true, options: ['A', 'B'] })) }], required_documents: [{ key: 'safety_plan', label: 'Safety plan', category: 'application' }] });
    expect(parsed.sections[0].fields.map(f => f.type)).toEqual(kinds);
    expect(parsed.sections[0].fields.every(f => f.required)).toBe(true);
    expect(parsed.documents[0]).toMatchObject({ key: 'safety_plan', label: 'Safety plan' });
  });

  it('renders a structured revision value without losing its contents', () => {
    expect(answerToText({ start: '2026-10-01', end: '2026-10-10' })).toContain('2026-10-10');
  });
});
