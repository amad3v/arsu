/**
 * A code split into two halves for reading and typing: "123 456",
 * "123 4567", "1234 5678".
 */
export function groupDigits(code: string): string {
  const half = Math.floor(code.length / 2);
  return `${code.slice(0, half)} ${code.slice(half)}`;
}

/** What stands in for a code not fetched yet: the same shape, in dots. */
export function codePlaceholder(digits: number): string {
  return groupDigits('•'.repeat(digits));
}
