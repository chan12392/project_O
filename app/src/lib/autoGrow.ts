import { useLayoutEffect, type RefObject } from "react";

/**
 * Grows a textarea with its text up to its CSS max-height, then it scrolls. A line break typed
 * at the end keeps that line in view: `setRangeText` (Ctrl+Enter) does not scroll by itself.
 */
export function useAutoGrow(ref: RefObject<HTMLTextAreaElement | null>, value: string): void {
  useLayoutEffect(() => {
    const input = ref.current;
    if (!input) return;
    input.style.height = "auto";
    input.style.height = `${input.scrollHeight}px`;
    if (input.selectionEnd === input.value.length) input.scrollTop = input.scrollHeight;
  }, [ref, value]);
}
