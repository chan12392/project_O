import type { AnchorHTMLAttributes } from "react";

import { backend } from "../platform/backend/index.ts";

type Props = Omit<AnchorHTMLAttributes<HTMLAnchorElement>, "href" | "target" | "rel"> & { href: string };

/**
 * A link to a web page outside the app (a PR, a page an agent reported). The
 * desktop app's WebView opens no new windows, so a click goes through the
 * backend: `open_external` hands an https address to the person's browser, and
 * a browser tab opens a new tab without a way back to this one (noopener).
 * Anything but https is shown as text. Chat Markdown has its own copy of this.
 */
export function ExternalAnchor({ href, children, onClick, ...rest }: Props) {
  if (!/^https:\/\/\S+$/i.test(href)) return <span className={rest.className}>{children}</span>;
  return (
    <a
      {...rest}
      href={href}
      target="_blank"
      rel="noreferrer noopener"
      onClick={(e) => {
        onClick?.(e);
        if (e.defaultPrevented) return;
        e.preventDefault();
        void backend().openExternal(href).catch(() => {});
      }}
    >
      {children}
    </a>
  );
}
