import { useState } from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FillFromText } from './FillFromText';

function Example() {
  const [answers, setAnswers] = useState<Record<string, unknown>>({});
  return <><FillFromText templateVersionId="template-1" fields={[{ key: 'objectives', label: 'Objectives', type: 'textarea' }]} onAccept={suggestions => setAnswers(previous => ({ ...previous, ...Object.fromEntries(suggestions.map(s => [s.field_key, s.value])) }))} /><output>{String(answers.objectives ?? '')}</output></>;
}

describe('Fill from text', () => {
  beforeEach(() => vi.unstubAllGlobals());
  afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

  it('accepts a suggestion into form state only after the user clicks Accept', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ suggestions: [{ field_key: 'objectives', value: 'Survey coral reefs', confidence: 0.9, source_excerpt: 'Objectives: survey coral reefs' }] }), { status: 200 })));
    render(<Example />);
    fireEvent.click(screen.getByText('Fill from text'));
    fireEvent.change(screen.getByRole('textbox', { name: 'Proposal text' }), { target: { value: 'Objectives: survey coral reefs' } });
    fireEvent.click(screen.getByRole('button', { name: 'Get suggested answers' }));
    expect(await screen.findByText('Survey coral reefs')).toBeInTheDocument();
    expect(screen.getByRole('status')).toBeEmptyDOMElement();
    fireEvent.click(screen.getByRole('button', { name: /^Accept$/ }));
    expect(screen.getByRole('status')).toHaveTextContent('Survey coral reefs');
  });

  it('hides the panel when AI is unavailable', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: { code: 'ai_unavailable', message: 'AI disabled' } }), { status: 503 })));
    render(<Example />);
    fireEvent.click(screen.getByText('Fill from text'));
    fireEvent.change(screen.getByRole('textbox', { name: 'Proposal text' }), { target: { value: 'A proposal' } });
    fireEvent.click(screen.getByRole('button', { name: 'Get suggested answers' }));
    await waitFor(() => expect(screen.queryByText('Fill from text')).not.toBeInTheDocument());
  });
});
