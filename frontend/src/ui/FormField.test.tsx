import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Checkbox } from './FormField';

describe('Checkbox', () => {
  it('associates its label outside a FormField', () => {
    render(<Checkbox label="This is fictional test data" />);
    expect(screen.getByRole('checkbox', { name: 'This is fictional test data' })).toBeInTheDocument();
  });
});
