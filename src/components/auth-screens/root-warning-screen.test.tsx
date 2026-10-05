import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library';
import { describe, expect, it, vi } from 'vitest';

import { appError, mockCommands } from '@cpt/testing/ipc';

import { RootWarningScreen } from '.';

vi.mock('@cpt/touch-ui', () => ({ isTouchUi: () => true }));

describe('RootWarningScreen', () => {
  it('closes the app when the user leaves', async () => {
    const calls = mockCommands({ answer_root_warning: () => null });
    const onAccepted = vi.fn();
    render(() => <RootWarningScreen onAccepted={onAccepted} />);

    fireEvent.click(screen.getByRole('button', { name: 'Close Arsu' }));

    await waitFor(() =>
      expect(calls).toEqual([{ cmd: 'answer_root_warning', args: { accept: false } }]),
    );
    expect(onAccepted).not.toHaveBeenCalled();
  });

  it('goes on once the user accepts the risk', async () => {
    const calls = mockCommands({ answer_root_warning: () => null });
    const onAccepted = vi.fn();
    render(() => <RootWarningScreen onAccepted={onAccepted} />);

    fireEvent.click(screen.getByRole('button', { name: 'I understand the risks, continue' }));

    await waitFor(() => expect(onAccepted).toHaveBeenCalledOnce());
    expect(calls).toEqual([{ cmd: 'answer_root_warning', args: { accept: true } }]);
  });

  it('stays, and says why, when the consent cannot be stored', async () => {
    mockCommands({
      answer_root_warning: () => {
        throw appError('FileWrite', 'could not write');
      },
    });
    const onAccepted = vi.fn();
    render(() => <RootWarningScreen onAccepted={onAccepted} />);

    fireEvent.click(screen.getByRole('button', { name: 'I understand the risks, continue' }));

    expect(await screen.findByText('could not write')).toBeTruthy();
    expect(onAccepted).not.toHaveBeenCalled();
  });
});
