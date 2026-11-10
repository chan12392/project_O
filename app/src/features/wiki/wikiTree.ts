/**
 * The Wiki tab's document list as a folder tree (ADR-0041). Pure, so the rules are
 * testable without a browser.
 */
import type { DreamDoc, DreamKind } from "../../platform/backend/types.ts";

export interface WikiFolder {
  /** The folder's own name ("" for the top) and its path from the top. */
  name: string;
  path: string;
  folders: WikiFolder[];
  docs: DreamDoc[];
}

/** The top folders in the order they are listed: what the cards learned, what people put in, then what the cards did and what they tried in vain. */
export const TOP_FOLDERS = ["배운 것", "외부 자료", "작업 성공", "작업 실패", "도구 오류"];

export const KIND_WORD: Record<DreamKind, string> = { note: "배운 것", external: "외부 자료", done: "작업 성공", failed: "작업 실패", tool: "도구 오류" };

const topOrder = (name: string) => {
  const at = TOP_FOLDERS.indexOf(name);
  return at < 0 ? TOP_FOLDERS.length : at;
};

/** The documents under their folders: folders first (the top ones in their fixed order, the rest by name), documents newest first. */
export function buildTree(docs: DreamDoc[]): WikiFolder {
  const root: WikiFolder = { name: "", path: "", folders: [], docs: [] };
  const byPath = new Map<string, WikiFolder>([["", root]]);
  const folderOf = (path: string): WikiFolder => {
    const known = byPath.get(path);
    if (known) return known;
    const cut = path.lastIndexOf("/");
    const made: WikiFolder = { name: path.slice(cut + 1), path, folders: [], docs: [] };
    byPath.set(path, made);
    folderOf(cut < 0 ? "" : path.slice(0, cut)).folders.push(made);
    return made;
  };
  for (const doc of docs) {
    const cut = doc.path.lastIndexOf("/");
    folderOf(cut < 0 ? "" : doc.path.slice(0, cut)).docs.push(doc);
  }
  for (const folder of byPath.values()) {
    folder.folders.sort((a, b) => (folder === root ? topOrder(a.name) - topOrder(b.name) : 0) || a.name.localeCompare(b.name));
    folder.docs.sort((a, b) => b.at.localeCompare(a.at) || a.path.localeCompare(b.path));
  }
  return root;
}

/** How many documents a folder holds, its sub-folders included. */
export function countDocs(folder: WikiFolder): number {
  return folder.docs.length + folder.folders.reduce((sum, sub) => sum + countDocs(sub), 0);
}

/**
 * The documents whose title, path, project or tags hold every word of the query (any case).
 * A word written as `#tag` asks for exactly that tag.
 */
export function filterDocs(docs: DreamDoc[], query: string): DreamDoc[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return docs;
  return docs.filter((doc) => {
    const tags = doc.tags ?? [];
    const text = `${doc.path}\n${doc.title}\n${doc.project ?? ""}\n${tags.join("\n")}`.toLowerCase();
    return words.every((word) => (word.startsWith("#") && word.length > 1 ? tags.includes(word.slice(1)) : text.includes(word)));
  });
}

/** The tags the documents carry, the most used first (then by name). */
export function tagCounts(docs: DreamDoc[]): { tag: string; count: number }[] {
  const counts = new Map<string, number>();
  for (const doc of docs) for (const tag of doc.tags ?? []) counts.set(tag, (counts.get(tag) ?? 0) + 1);
  return [...counts].map(([tag, count]) => ({ tag, count })).sort((a, b) => b.count - a.count || a.tag.localeCompare(b.tag));
}

/** The query with `#tag` added, or taken out when it is already asked for. */
export function toggleTag(query: string, tag: string): string {
  const word = `#${tag}`;
  const words = query.split(/\s+/).filter(Boolean);
  const kept = words.filter((w) => w.toLowerCase() !== word);
  return (kept.length === words.length ? [...words, word] : kept).join(" ");
}

/** A document's file name without its folders. */
export const fileName = (path: string) => path.slice(path.lastIndexOf("/") + 1);

/** The document the Wiki opens first: the newest one of all. */
export function firstDoc(docs: DreamDoc[]): string | undefined {
  return docs.reduce<DreamDoc | undefined>((newest, doc) => (!newest || doc.at > newest.at ? doc : newest), undefined)?.path;
}

/** The listed document a link written in a document names by its path, or undefined (a web address, an anchor, anything else). */
export function resolveLink(from: string, href: string, paths: readonly string[]): string | undefined {
  if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("#") || href.startsWith("//")) return undefined;
  let target = href.split(/[?#]/)[0].replace(/\\/g, "/").replace(/^\/+/, "");
  try {
    target = decodeURIComponent(target);
  } catch {
    // A stray "%" is just part of the name.
  }
  return target && target !== from && paths.includes(target) ? target : undefined;
}

/** A text file's name as a material's title: no folders, no extension. */
export function titleFromFile(name: string): string {
  const file = name.replace(/\\/g, "/").split("/").pop() ?? name;
  return file.replace(/\.[A-Za-z0-9]{1,8}$/, "") || file;
}
