import { render, screen } from '@solidjs/testing-library';
import { describe, expect, it, vi } from 'vitest';

import { ImportResult } from './import-result';

import type { ImportReport } from './import-report';

const report: ImportReport = { headline: 'Imported 3 entries.', warning: null, sections: [] };

describe('ImportResult', () => {
  it('ends with a Done button in the dialog', () => {
    const onDone = vi.fn();
    render(() => <ImportResult report={report} onDone={onDone} />);

    screen.getByRole('button', { name: 'Done' }).click();
    expect(onDone).toHaveBeenCalledOnce();
  });

  it('has a key by the headline and no Done button in a sheet, which a tap outside closes', () => {
    render(() => <ImportResult report={report} onDone={vi.fn()} inSheet />);

    const headline = screen.getByRole('heading', { name: 'Imported 3 entries.' });
    expect(headline.querySelector('.i-ph-key-bold')).not.toBeNull();
    expect(screen.queryByRole('button', { name: 'Done' })).toBeNull();
  });
});
