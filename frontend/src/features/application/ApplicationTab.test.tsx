import { useState } from 'react';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { FieldControl } from './ApplicationTab';

describe('application field controls', () => {
  it('keeps named people on separate lines while editing', () => {
    function Example() {
      const [people, setPeople] = useState<unknown>([]);
      return <FieldControl field={{ key: 'researchers', label: 'Researchers', type: 'people' }} value={people} change={setPeople} disabled={false} />;
    }
    render(<Example />);
    const input = screen.getByRole('textbox');
    fireEvent.change(input, { target: { value: 'Anna Hart\nLiam Chen' } });
    expect(input).toHaveValue('Anna Hart\nLiam Chen');
  });

  it('allows multiple choices without dropping the previous choice', () => {
    function Example() {
      const [choices, setChoices] = useState<unknown>([]);
      return <FieldControl field={{ key: 'methods', label: 'Methods', type: 'multiselect', options: ['Diving', 'Survey'] }} value={choices} change={setChoices} disabled={false} />;
    }
    render(<Example />);
    fireEvent.click(screen.getByLabelText('Diving'));
    fireEvent.click(screen.getByLabelText('Survey'));
    expect(screen.getByLabelText('Diving')).toBeChecked();
    expect(screen.getByLabelText('Survey')).toBeChecked();
  });
});
