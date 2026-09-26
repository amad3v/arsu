import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { scanQrInWorker } from './scan-qr';

/** Stands in for the QR worker: the test replies or fails for it. */
class FakeWorker extends EventTarget {
  static created: FakeWorker[] = [];
  readonly posted: unknown[] = [];
  terminated = false;

  constructor(
    readonly url: string | URL,
    readonly options?: WorkerOptions,
  ) {
    super();
    FakeWorker.created.push(this);
  }

  postMessage(message: unknown) {
    this.posted.push(message);
  }

  terminate() {
    this.terminated = true;
  }

  reply(data: unknown) {
    this.dispatchEvent(new MessageEvent('message', { data }));
  }

  crash() {
    this.dispatchEvent(new Event('error'));
  }
}

/** The worker the scan started: each scan starts exactly one. */
function lastWorker(): FakeWorker {
  expect(FakeWorker.created).toHaveLength(1);
  return FakeWorker.created[0];
}

describe('scanQrInWorker', () => {
  const image = new Blob(['png'], { type: 'image/png' });

  beforeEach(() => {
    FakeWorker.created = [];
    vi.stubGlobal('Worker', FakeWorker);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('runs the bundled QR worker as a module and sends it the image', () => {
    void scanQrInWorker(image, new AbortController().signal);

    const worker = lastWorker();
    expect(String(worker.url)).toMatch(/\/qr-worker\.ts\b/);
    expect(worker.options).toEqual({ type: 'module' });
    expect(worker.posted).toEqual([image]);
  });

  it("resolves with the worker's reply and stops the worker", async () => {
    const scan = scanQrInWorker(image, new AbortController().signal);

    lastWorker().reply({ status: 'found', text: 'otpauth://totp/x?secret=A' });

    await expect(scan).resolves.toEqual({ status: 'found', text: 'otpauth://totp/x?secret=A' });
    expect(lastWorker().terminated).toBe(true);
  });

  it('stops the worker and rejects when cancelled', async () => {
    const cancel = new AbortController();
    const scan = scanQrInWorker(image, cancel.signal);

    cancel.abort();

    await expect(scan).rejects.toMatchObject({ name: 'AbortError' });
    expect(lastWorker().terminated).toBe(true);
  });

  it('starts no worker when already cancelled', async () => {
    const cancel = new AbortController();
    cancel.abort();

    await expect(scanQrInWorker(image, cancel.signal)).rejects.toMatchObject({
      name: 'AbortError',
    });
    expect(FakeWorker.created).toEqual([]);
  });

  it('rejects when the worker cannot run', async () => {
    const scan = scanQrInWorker(image, new AbortController().signal);

    lastWorker().crash();

    await expect(scan).rejects.toThrow('The QR code reader could not start.');
    expect(lastWorker().terminated).toBe(true);
  });
});
