/**
 * The value of a whole number typed in decimal digits (surrounding spaces
 * allowed), or null for anything else: an empty field, a sign, a fraction, an
 * exponent, or a number too large to represent exactly. Stricter than
 * `Number.parseInt`, which reads "30s" as 30 and "1e3" as 1.
 */
export function parseWholeNumber(text: string): number | null {
  const digits = text.trim();
  if (!/^\d+$/.test(digits)) return null;
  const value = Number(digits);
  return Number.isSafeInteger(value) ? value : null;
}
