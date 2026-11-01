import { ArrowUp } from "lucide-react";
import { useState, type FormEvent, type KeyboardEvent, type RefObject } from "react";

const MAC = typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");
const HELP = `총괄이 작업카드로 나눠 O에 전달합니다. ${MAC ? "⌘" : "Ctrl+"}Enter로 보내기`;

/**
 * Orca's chat composer. The browser preview has no lead session behind it, so
 * it says plainly that nothing was sent; the desktop build wires `data-endpoint`.
 */
export function Composer({ inputRef }: { inputRef: RefObject<HTMLTextAreaElement | null> }) {
  const [text, setText] = useState("");
  const [help, setHelp] = useState(HELP);

  const submit = (event?: FormEvent) => {
    event?.preventDefault();
    if (!text.trim()) return;
    setHelp("보내지 않았습니다. 미리보기는 총괄 세션에 연결되어 있지 않습니다.");
  };
  const onKey = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && (MAC ? event.metaKey : event.ctrlKey)) submit();
  };

  return (
    <form className="shrink-0 bg-chat-canvas px-6 pb-4" data-endpoint="" noValidate onSubmit={submit}>
      <div className="mx-auto max-w-3xl rounded-xl border border-chat-composer-border bg-chat-composer-surface px-3 pt-2.5 pb-2 focus-within:ring-2 focus-within:ring-ring/50">
        <label htmlFor="ask" className="sr-only">
          총괄에게 맡길 일
        </label>
        <textarea
          id="ask"
          ref={inputRef}
          name="ask"
          spellCheck={false}
          rows={2}
          value={text}
          placeholder="총괄에게 맡길 일을 적어 주세요"
          aria-describedby="ask-help"
          onChange={(e) => {
            setText(e.target.value);
            if (help !== HELP) setHelp(HELP);
          }}
          onKeyDown={onKey}
          className="block max-h-48 w-full resize-none bg-transparent text-sm text-chat-foreground outline-none placeholder:text-chat-foreground-faint"
        />
        <div className="mt-1.5 flex items-center gap-2">
          <p id="ask-help" aria-live="polite" className="min-w-0 flex-1 truncate text-xs text-chat-foreground-faint">
            {help}
          </p>
          <button
            type="submit"
            aria-label="맡기기"
            disabled={!text.trim()}
            className="flex size-7 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground hover:bg-primary/90 focus-ring disabled:cursor-not-allowed disabled:opacity-40"
          >
            <ArrowUp className="size-4" aria-hidden="true" />
          </button>
        </div>
      </div>
    </form>
  );
}
