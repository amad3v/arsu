import { render, screen } from '@solidjs/testing-library';
import { describe, expect, it } from 'vitest';

import { NumberField, SelectField, TextField } from '.';

function describedBy(element: HTMLElement): string[] {
  return element.getAttribute('aria-describedby')?.split(' ') ?? [];
}

describe('TextField', () => {
  it('labels its input and describes it with the helper text', () => {
    render(() => (
      <TextField
        label={'Secret key'}
        description={'As the service shows it.'}
        value={''}
        onValueChange={() => undefined}
      />
    ));

    const input = screen.getByLabelText('Secret key');
    expect(describedBy(input)).toEqual([screen.getByText('As the service shows it.').id]);
    expect(input.hasAttribute('aria-invalid')).toBe(false);
  });

  it('marks an error on the input and links it, so it is read with the field', () => {
    render(() => (
      <TextField
        label={'Secret key'}
        description={'As the service shows it.'}
        error={'Enter the secret key.'}
        value={''}
        onValueChange={() => undefined}
      />
    ));

    const input = screen.getByLabelText('Secret key');
    const error = screen.getByText('Enter the secret key.');
    expect(input.getAttribute('aria-invalid')).toBe('true');
    expect(describedBy(input)).toContain(error.id);
    expect(error.getAttribute('aria-live')).toBe('polite');
  });

  it('turns off autocomplete history and spell checking', () => {
    render(() => <TextField label={'Issuer'} value={''} onValueChange={() => undefined} />);

    const input = screen.getByLabelText('Issuer');
    expect(input.getAttribute('autocomplete')).toBe('off');
    expect(input.getAttribute('spellcheck')).toBe('false');
  });
});

describe('NumberField', () => {
  it('labels its input and links its error', () => {
    render(() => (
      <NumberField
        label={'Period (seconds)'}
        value={'0'}
        onValueChange={() => undefined}
        error={'Use a whole number of seconds from 1 to 300.'}
        min={1}
        max={300}
      />
    ));

    const input = screen.getByLabelText('Period (seconds)');
    const error = screen.getByText('Use a whole number of seconds from 1 to 300.');
    expect(input.getAttribute('aria-invalid')).toBe('true');
    expect(describedBy(input)).toContain(error.id);
  });

  it('names its stepper buttons', () => {
    render(() => (
      <NumberField label={'Counter'} value={'0'} onValueChange={() => undefined} min={0} max={9} />
    ));

    expect(screen.getAllByRole('button').every((button) => button.getAttribute('aria-label'))).toBe(
      true,
    );
  });
});

describe('SelectField', () => {
  it('is named by its label and shows the chosen option', () => {
    render(() => (
      <SelectField
        label={'Algorithm'}
        options={[
          { value: 'sha1', label: 'SHA-1' },
          { value: 'sha256', label: 'SHA-256' },
        ]}
        value={'sha256'}
        onValueChange={() => undefined}
      />
    ));

    const trigger = screen.getByRole('combobox', { name: /Algorithm/ });
    expect(trigger.textContent).toContain('SHA-256');
  });
});
