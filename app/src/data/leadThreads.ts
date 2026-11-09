import { folderKey } from "../../../src/core/projects.ts";
import { readStored, writeStored } from "../lib/storage.ts";

const KEY = "project-o-lead-tabs";

const load = (): Record<string, string> =>
  readStored<Record<string, string>>(KEY, {}, (saved) => (saved && typeof saved === "object" && !Array.isArray(saved) ? (saved as Record<string, string>) : undefined));

/**
 * The lead tab last looked at in a project (ADR-0034): remembered so switching back to the
 * project opens the same conversation, and shared with the panel window through the selection.
 * A pick whose tab was closed is dropped the next time the project's answer lands.
 */
export function rememberedThread(project: string): string | undefined {
  return load()[folderKey(project)];
}

export function rememberThread(project: string, thread: string | undefined): void {
  const saved = load();
  const key = folderKey(project);
  if (thread === undefined) delete saved[key];
  else saved[key] = thread;
  writeStored(KEY, saved);
}
