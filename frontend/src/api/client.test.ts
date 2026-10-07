import { describe, expect, it } from 'vitest';
import { ApiError, parseErrorBody } from './client';

describe('API error parsing', () => {
  it('keeps code, message, status and field errors', () => {
    const error = parseErrorBody(422, { error: { code: 'invalid_input', message: 'Check fields', fields: { title: 'Required' } } });
    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({ status: 422, code: 'invalid_input', message: 'Check fields', fields: { title: 'Required' } });
    expect(error.fieldError('title')).toBe('Required');
  });
  it('handles an invalid or empty body', () => {
    expect(parseErrorBody(503, null)).toMatchObject({ status: 503, code: 'http_503', message: 'Request failed (503)' });
  });
});
