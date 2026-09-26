import { afterEach, describe, expect, it, vi } from 'vitest';

import { decodeSize, MAX_DECODE_PIXELS, readQrCode, scanQrImage } from './qr-image';

import type { PixelSize } from './qr-image';

const FIXTURE_TEXT = 'otpauth://totp/A:b?secret=JBSWY3DPEHPK3PXP';

/** `qrencode -l L -m 0` of FIXTURE_TEXT: '#' is a dark module. */
const FIXTURE_MODULES = [
  '#######...#..####..#..#######',
  '#.....#..#...#.#...##.#.....#',
  '#.###.#.#..#..###.###.#.###.#',
  '#.###.#..#..#..##.##..#.###.#',
  '#.###.#..#..####..##..#.###.#',
  '#.....#...###.#..####.#.....#',
  '#######.#.#.#.#.#.#.#.#######',
  '........#..#.###.####........',
  '###.#####...####.#..###...#..',
  '..#.#...##.####.###.#.#..#.##',
  '..#..##.#.###.#.#....###.####',
  '######..###.##.#.#...#..#..#.',
  '......##..##..#####..##..#..#',
  '##..##...###......#.#.#.....#',
  '##.#.###.##..#........#.#####',
  '#.##.#.#..##...#.##.##.....#.',
  '##.#####..#.#....###..##.#.##',
  '.####..##..###...#...##.....#',
  '#.#..##.#####.#...#.##.#..###',
  '.###...#....#####.....#.#...#',
  '#.#.#.###.##..#...#.#####..#.',
  '........#.##..#.##.##...#####',
  '#######.###..#.#.####.#.#.###',
  '#.....#.##.#....###.#...##.##',
  '#.###.#.#.#.#....#..######...',
  '#.###.#...####.#.#.##..####..',
  '#.###.#.#..##.##.##...#.#...#',
  '#.....#.#...####....##..##.#.',
  '#######.#.##..#.###.####...##',
];

/** The fixture as RGBA pixels: a 4-module quiet zone, 4 pixels per module. */
function fixturePixels(inverted = false): { pixels: Uint8ClampedArray; size: PixelSize } {
  const scale = 4;
  const quiet = 4;
  const count = FIXTURE_MODULES.length;
  const side = (count + 2 * quiet) * scale;
  const pixels = new Uint8ClampedArray(side * side * 4);
  const inCode = (module: number) => module >= 0 && module < count;

  for (let y = 0; y < side; y += 1) {
    for (let x = 0; x < side; x += 1) {
      const row = Math.floor(y / scale) - quiet;
      const column = Math.floor(x / scale) - quiet;
      const dark = inCode(row) && inCode(column) && FIXTURE_MODULES[row][column] === '#';
      const value = dark !== inverted ? 0 : 255;
      pixels.set([value, value, value, 255], (y * side + x) * 4);
    }
  }
  return { pixels, size: { width: side, height: side } };
}

describe('decodeSize', () => {
  it('keeps an image that is small enough', () => {
    expect(decodeSize(1920, 1080)).toEqual({ width: 1920, height: 1080 });
  });

  it('scales a large image down to the pixel budget, keeping its shape', () => {
    const size = decodeSize(7680, 4320);

    expect(size).not.toBeNull();
    if (size === null) return;
    expect(size.width * size.height).toBeLessThanOrEqual(MAX_DECODE_PIXELS);
    expect(size.width / size.height).toBeCloseTo(7680 / 4320, 2);
  });

  it('never scales a side below one pixel, and still keeps to the budget', () => {
    expect(decodeSize(100_000_000, 1)).toEqual({ width: MAX_DECODE_PIXELS, height: 1 });
    expect(decodeSize(1, 100_000_000)).toEqual({ width: 1, height: MAX_DECODE_PIXELS });
  });

  it.each([
    [0, 100],
    [100, 0],
    [-1, 100],
    [Number.NaN, 100],
    [Number.POSITIVE_INFINITY, 100],
  ])('rejects an image with no area: %d×%d', (width, height) => {
    expect(decodeSize(width, height)).toBeNull();
  });
});

describe('readQrCode', () => {
  it('reads a dark-on-light code', () => {
    const { pixels, size } = fixturePixels();

    expect(readQrCode(pixels, size)).toBe(FIXTURE_TEXT);
  });

  it('reads an inverted code, as in a dark-mode screenshot', () => {
    const { pixels, size } = fixturePixels(true);

    expect(readQrCode(pixels, size)).toBe(FIXTURE_TEXT);
  });

  it('is null when there is no code', () => {
    const size = { width: 64, height: 64 };

    expect(readQrCode(new Uint8ClampedArray(64 * 64 * 4).fill(255), size)).toBeNull();
  });
});

describe('scanQrImage', () => {
  /** Stands in for the worker's createImageBitmap and OffscreenCanvas. */
  function stubCanvas(bitmap: { width: number; height: number }, pixels: Uint8ClampedArray) {
    const close = vi.fn();
    const drawImage =
      vi.fn<(image: unknown, x: number, y: number, width: number, height: number) => void>();
    vi.stubGlobal(
      'createImageBitmap',
      vi.fn(() => Promise.resolve({ ...bitmap, close })),
    );
    vi.stubGlobal(
      'OffscreenCanvas',
      class {
        getContext() {
          return { drawImage, getImageData: () => ({ data: pixels }) };
        }
      },
    );
    return { close, drawImage };
  }

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('finds the code in the image and releases the bitmap', async () => {
    const { pixels, size } = fixturePixels();
    const { close } = stubCanvas(size, pixels);

    await expect(scanQrImage(new Blob())).resolves.toEqual({
      status: 'found',
      text: FIXTURE_TEXT,
    });
    expect(close).toHaveBeenCalledOnce();
  });

  it('reports an image without a code', async () => {
    stubCanvas({ width: 8, height: 8 }, new Uint8ClampedArray(8 * 8 * 4).fill(255));

    await expect(scanQrImage(new Blob())).resolves.toEqual({ status: 'not-found' });
  });

  it('draws a large image scaled down', async () => {
    const { drawImage } = stubCanvas(
      { width: 8000, height: 8000 },
      new Uint8ClampedArray(4).fill(255),
    );

    await scanQrImage(new Blob());

    expect(drawImage).toHaveBeenCalledOnce();
    const [, , , width = 0, height = 0] = drawImage.mock.lastCall ?? [];
    expect(width).toBeGreaterThan(0);
    expect(width * height).toBeLessThanOrEqual(MAX_DECODE_PIXELS);
  });

  it('reports an image with no pixels as unreadable', async () => {
    const { close } = stubCanvas({ width: 0, height: 0 }, new Uint8ClampedArray());

    await expect(scanQrImage(new Blob())).resolves.toEqual({ status: 'unreadable' });
    expect(close).toHaveBeenCalledOnce();
  });

  it('reports a file the engine cannot decode as unreadable', async () => {
    vi.stubGlobal(
      'createImageBitmap',
      vi.fn(() => Promise.reject(new DOMException('bad image', 'InvalidStateError'))),
    );

    await expect(scanQrImage(new Blob(['not an image']))).resolves.toEqual({
      status: 'unreadable',
    });
  });
});
