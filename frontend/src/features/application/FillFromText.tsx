import { useState } from 'react';
import { apiPost } from '../../api/client';
import type { AssistExtractRequest } from '../../api/generated/AssistExtractRequest';
import type { AssistExtractResponse } from '../../api/generated/AssistExtractResponse';
import type { AssistSuggestionDto } from '../../api/generated/AssistSuggestionDto';
import { Banner, Button, FormField, Textarea } from '../../ui';
import { answerToText, type FormFieldSchema } from './schema';

export function FillFromText({ templateVersionId, fields, onAccept }: { templateVersionId: string; fields: FormFieldSchema[]; onAccept: (suggestions: AssistSuggestionDto[]) => void }) {
  const [text, setText] = useState('');
  const [suggestions, setSuggestions] = useState<AssistSuggestionDto[]>([]);
  const [loading, setLoading] = useState(false);
  const [unavailable, setUnavailable] = useState(false);
  const [error, setError] = useState('');
  if (unavailable) return null;
  async function extract() {
    setLoading(true); setError('');
    try {
      const result = await apiPost<AssistExtractResponse>('/assist/extract-fields', { template_version_id: templateVersionId, text } satisfies AssistExtractRequest);
      setSuggestions(result.suggestions.filter(s => fields.some(f => f.key === s.field_key)));
    } catch (e) {
      if (e && typeof e === 'object' && 'status' in e && 'code' in e && e.status === 503 && e.code === 'ai_unavailable') setUnavailable(true);
      else setError(e instanceof Error ? e.message : 'Could not extract suggestions.');
    } finally { setLoading(false); }
  }
  function accept(selected: AssistSuggestionDto[]) {
    onAccept(selected);
    const keys = new Set(selected.map(s => s.field_key));
    setSuggestions(current => current.filter(s => !keys.has(s.field_key)));
  }
  return <details className="rounded-lg border border-teal-200 bg-teal-50 p-4">
    <summary className="cursor-pointer font-semibold text-teal-950">Fill from text</summary>
    <div className="mt-4 space-y-4">
      <p className="text-sm text-teal-900">Paste text from an existing proposal and get suggested answers (AI suggestion — review before accepting).</p>
      <FormField label="Proposal text"><Textarea value={text} onChange={e => setText(e.target.value)} rows={7} /></FormField>
      <Button onClick={() => void extract()} loading={loading} disabled={!text.trim()}>Get suggested answers</Button>
      {error && <Banner tone="error">{error}</Banner>}
      {suggestions.length > 0 && <div className="space-y-3" aria-label="Suggested answers">
        <Button variant="secondary" onClick={() => accept(suggestions.filter(s => s.confidence >= 0.8))} disabled={!suggestions.some(s => s.confidence >= 0.8)}>Accept all high-confidence</Button>
        {suggestions.map(s => <div key={s.field_key} className="rounded-md border border-teal-200 bg-white p-3">
          <h3 className="font-semibold text-navy-900">{fields.find(f => f.key === s.field_key)?.label}</h3>
          <p className="whitespace-pre-wrap text-sm text-slate-800">{answerToText(s.value)}</p>
          <p className="mt-1 text-xs text-slate-600">Confidence: {Math.round(s.confidence * 100)}% · Source: “{s.source_excerpt}”</p>
          <div className="mt-2 flex gap-2"><Button size="sm" onClick={() => accept([s])}>Accept</Button><Button size="sm" variant="secondary" onClick={() => setSuggestions(current => current.filter(item => item.field_key !== s.field_key))}>Dismiss</Button></div>
        </div>)}
      </div>}
    </div>
  </details>;
}
