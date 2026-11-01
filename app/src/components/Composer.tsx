import { useState, type FormEvent, type KeyboardEvent, type RefObject } from "react";

const HELP = "보내면 총괄이 작업카드로 나눠 O에 전달합니다. Ctrl+Enter로도 보낼 수 있습니다.";

/**
 * The request box. The browser preview has no lead session behind it, so it
 * says plainly that nothing was sent; the desktop build wires `data-endpoint`.
 */
export function Composer({ inputRef }: { inputRef: RefObject<HTMLTextAreaElement | null> }) {
  const [text, setText] = useState("");
  const [help, setHelp] = useState(HELP);
  const [invalid, setInvalid] = useState(false);

  const submit = (event?: FormEvent) => {
    event?.preventDefault();
    if (!text.trim()) {
      setInvalid(true);
      setHelp("맡길 일을 한 줄 이상 적어 주세요.");
      inputRef.current?.focus();
      return;
    }
    setInvalid(false);
    setHelp("보내지 않았습니다. 미리보기는 총괄 세션에 연결되어 있지 않습니다.");
  };

  const onKey = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) submit();
  };

  return (
    <form className="composer" data-endpoint="" noValidate onSubmit={submit}>
      <label htmlFor="ask">총괄에게 맡길 일</label>
      <div className="composer__row">
        <textarea
          id="ask"
          ref={inputRef}
          name="ask"
          className="input"
          rows={3}
          value={text}
          aria-invalid={invalid || undefined}
          aria-describedby="ask-help"
          onChange={(e) => {
            setText(e.target.value);
            if (invalid) {
              setInvalid(false);
              setHelp(HELP);
            }
          }}
          onKeyDown={onKey}
        />
        <button type="submit" className="btn btn--primary">
          맡기기
        </button>
      </div>
      <p id="ask-help" className={`composer__help${help === HELP ? " is-default" : ""}`} aria-live="polite">
        {help}
      </p>
    </form>
  );
}
