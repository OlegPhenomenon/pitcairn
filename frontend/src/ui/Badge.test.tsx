import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { STATUS_MAP, StatusBadge } from './Badge';

describe('StatusBadge', () => {
  it('uses clear labels and expected tones for key states', () => {
    expect(STATUS_MAP.changes_requested).toEqual({ label: 'Needs your reply', tone: 'amber' });
    expect(STATUS_MAP.refused.tone).toBe('red');
    expect(STATUS_MAP.accepted.tone).toBe('green');
    render(<StatusBadge status="changes_requested" />);
    expect(screen.getByText('Needs your reply')).toHaveClass('bg-amber-50');
  });
});
