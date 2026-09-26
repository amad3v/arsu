// Wording helpers shared by the screens. The interface is English only.

const pluralRules = new Intl.PluralRules('en');

/** The count with the noun form English uses for it: "1 entry", "3 entries". */
export function countLabel(count: number, one: string, other: string): string {
  return `${count} ${pluralRules.select(count) === 'one' ? one : other}`;
}
