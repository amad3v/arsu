// Wording helpers shared by the screens. The interface is English only.

const pluralRules = new Intl.PluralRules('en');

/** The count with the noun form English uses for it: "1 entry", "3 entries". */
export function countLabel(count: number, one: string, other: string): string {
  return `${count} ${pluralRules.select(count) === 'one' ? one : other}`;
}

/** A file's size as people read it: "640 bytes", "12.3 kB", "1.2 MB" (decimal units, as file managers show them). */
export function fileSize(bytes: number): string {
  if (bytes < 1000) return countLabel(bytes, 'byte', 'bytes');
  const [size, unit] = bytes < 1_000_000 ? [bytes / 1000, 'kB'] : [bytes / 1_000_000, 'MB'];
  return `${size.toFixed(1)} ${unit}`;
}
