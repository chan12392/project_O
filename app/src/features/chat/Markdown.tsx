import { Image as ImageIcon } from "lucide-react";
import { memo, type MouseEvent } from "react";
import ReactMarkdown, { type Components } from "react-markdown";

import { backend } from "../../platform/backend/index.ts";
import { CHAT_MARKDOWN } from "./markdown.ts";
import { ImageGallery } from "./ImageGallery.tsx";

/** In the desktop app a link opens in the person's browser (open_external); a browser tab opens it itself. */
function openOutside(e: MouseEvent<HTMLAnchorElement>) {
  if (!window.__TAURI_INTERNALS__) return;
  e.preventDefault();
  void backend().openExternal(e.currentTarget.href).catch(() => {});
}

const COMPONENTS: Components = {
  // Hrefs reach here only through safeUrl; anything else is plain text.
  a: ({ node, href, children }) => {
    if (!href) return <span>{children}</span>;
    const image = node?.properties.dataImage !== undefined;
    return (
      <a href={href} target="_blank" rel="noreferrer noopener" title={href} onClick={openOutside}>
        {image && (
          <>
            <ImageIcon className="md-image-icon" aria-hidden="true" />
            <span className="sr-only">이미지: </span>
          </>
        )}
        {children}
      </a>
    );
  },
  // Wide tables and long lines scroll inside the bubble; focusable so the keyboard can scroll them too.
  table: ({ children }) => (
    <div className="md-scroll" tabIndex={0}>
      <table>{children}</table>
    </div>
  ),
  pre: ({ children }) => (
    <pre className="md-scroll" tabIndex={0}>
      {children}
    </pre>
  ),
};

/**
 * Chat text as GitHub-flavoured Markdown: headings, lists, tables, code, quotes and
 * links (markdown.ts decides what is allowed). `inverse` is for text on the primary
 * colour, such as the lead's team-chat bubble.
 */
export const Markdown = memo(function Markdown({ text, inverse = false }: { text: string; inverse?: boolean }) {
  return (
    <div className={inverse ? "chat-markdown chat-markdown--inverse" : "chat-markdown"}>
      <ReactMarkdown {...CHAT_MARKDOWN} components={COMPONENTS}>
        {text}
      </ReactMarkdown>
      <ImageGallery text={text} />
    </div>
  );
});
