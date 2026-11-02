/**
 * What chat Markdown may become. Agent and PR text is untrusted, so: raw HTML stays
 * literal text (react-markdown without rehype-raw), only http(s) links get an href,
 * images are never fetched (alt text and a link instead), and a lead's `o-plan`
 * block is left to the plan card. Markdown.tsx draws the result.
 */
import type { Options } from "react-markdown";
import remarkBreaks from "remark-breaks";
import remarkGfm from "remark-gfm";

/** An http(s) address safe to open in the person's browser, or undefined (no link). */
export function safeUrl(url: string): string | undefined {
  let parsed: URL;
  try {
    parsed = new URL(url.trim());
  } catch {
    return undefined; // relative paths and anchors: nothing to open outside the app
  }
  if (parsed.protocol !== "https:" && parsed.protocol !== "http:") return undefined;
  // "https://trusted.com@elsewhere" reads as one site and opens another.
  if (parsed.username || parsed.password) return undefined;
  return parsed.href;
}

/** The prop that marks an image turned into a link (hast `dataImage`), so the link can say so. */
export const IMAGE_LINK = "data-image";

/** The hast nodes this file touches; react-markdown hands any tree to rehype plugins. */
interface HastNode {
  type: string;
  tagName?: string;
  value?: string;
  properties?: Record<string, unknown>;
  children?: HastNode[];
}

/** `<br>` is the one tag agents put in table cells on purpose; any other HTML stays text. */
const BREAK_TAG = /^<br\s*\/?>$/i;

function isPlanBlock(node: HastNode): boolean {
  const code = node.tagName === "pre" ? node.children?.find((c) => c.type === "element") : undefined;
  const classes = code?.tagName === "code" ? code.properties?.className : undefined;
  return Array.isArray(classes) && classes.includes("language-o-plan");
}

function imageAsLink(img: HastNode, inLink: boolean): HastNode {
  const alt = String(img.properties?.alt ?? "").trim() || "이미지";
  const text: HastNode = { type: "text", value: alt };
  // Inside a link the alt text is enough; links do not nest.
  if (inLink) return text;
  return { type: "element", tagName: "a", properties: { href: String(img.properties?.src ?? ""), dataImage: "" }, children: [text] };
}

/** Where an HTML block can stand: it reads as its own paragraph of literal text there. */
const BLOCK_PARENTS = new Set(["blockquote", "li"]);

function tidy(parent: HastNode, inLink: boolean): void {
  if (!parent.children) return;
  const blockLevel = parent.type === "root" || BLOCK_PARENTS.has(parent.tagName ?? "");
  parent.children = parent.children.flatMap((child): HastNode[] => {
    if (child.type === "raw" && BREAK_TAG.test(child.value?.trim() ?? "")) return [{ type: "element", tagName: "br", properties: {}, children: [] }];
    if (child.type === "raw" && blockLevel) return [{ type: "element", tagName: "p", properties: {}, children: [{ type: "text", value: child.value ?? "" }] }];
    if (child.type !== "element") return [child];
    if (isPlanBlock(child)) return [];
    if (child.tagName === "img") return [imageAsLink(child, inLink)];
    tidy(child, inLink || child.tagName === "a");
    return [child];
  });
}

/** Runs after Markdown becomes hast, before react-markdown checks URLs and drops raw HTML. */
function rehypeChat() {
  return (tree: HastNode) => tidy(tree, false);
}

/** The parser setup every chat bubble shares (Markdown.tsx adds the components). */
export const CHAT_MARKDOWN = {
  remarkPlugins: [remarkGfm, remarkBreaks],
  rehypePlugins: [rehypeChat],
  urlTransform: (url) => safeUrl(url),
} satisfies Options;
