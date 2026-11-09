/**
 * Small per-browser preferences in localStorage. Storage can be off (private
 * windows), so a failed read gives the fallback and a failed write is dropped:
 * the preference then lasts for this window only.
 */
export function readStored<T>(key: string, fallback: T, validate: (saved: unknown) => T | undefined): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return fallback;
    let saved: unknown;
    try {
      saved = JSON.parse(raw);
    } catch {
      // Older saves kept some choices as bare text.
      saved = raw;
    }
    return validate(saved) ?? fallback;
  } catch {
    return fallback;
  }
}

export function writeStored(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Storage off: kept for this window only.
  }
}
