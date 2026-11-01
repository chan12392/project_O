import type { ReactNode } from "react";

/**
 * The little Markdown agents write in chat: paragraphs, nested "- " lists,
 * **bold** and `code`. Built as React elements, never as HTML, so agent text
 * cannot inject markup.
 */
export function Markdown({ text }: { text: string }) {
  const blocks = text.trim().split(/\n{2,}/);
  return (
    <div className="space-y-2">
      {blocks.map((block, i) => (isList(block) ? <List key={i} lines={block.split("\n")} /> : <p key={i}>{inline(block)}</p>))}
    </div>
  );
}

function isList(block: string): boolean {
  return block.split("\n").every((line) => /^\s*- /.test(line));
}

interface Item {
  text: string;
  children: Item[];
}

function List({ lines }: { lines: string[] }) {
  const root: Item[] = [];
  const stack: { depth: number; items: Item[] }[] = [{ depth: -1, items: root }];
  for (const line of lines) {
    const depth = line.search(/\S/);
    while (stack.length > 1 && stack[stack.length - 1].depth >= depth) stack.pop();
    const item: Item = { text: line.replace(/^\s*- /, ""), children: [] };
    stack[stack.length - 1].items.push(item);
    stack.push({ depth, items: item.children });
  }
  return <Items items={root} />;
}

function Items({ items }: { items: Item[] }) {
  return (
    <ul className="list-disc space-y-1 pl-5 marker:text-chat-foreground-faint">
      {items.map((item, i) => (
        <li key={i}>
          {inline(item.text)}
          {item.children.length > 0 && <div className="mt-1">{<Items items={item.children} />}</div>}
        </li>
      ))}
    </ul>
  );
}

function inline(text: string): ReactNode[] {
  return text.split(/(\*\*[^*]+\*\*|`[^`]+`)/g).map((part, i) => {
    if (part.startsWith("**") && part.endsWith("**")) {
      return (
        <strong key={i} className="font-semibold text-chat-foreground-strong">
          {part.slice(2, -2)}
        </strong>
      );
    }
    if (part.startsWith("`") && part.endsWith("`")) {
      return (
        <code key={i} className="rounded border border-chat-inline-code-border bg-chat-inline-code-surface px-1 font-mono text-chat-code">
          {part.slice(1, -1)}
        </code>
      );
    }
    return part;
  });
}
