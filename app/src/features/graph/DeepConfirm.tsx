import { Sparkles } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";

import { BUTTON_PRIMARY, BUTTON_SECONDARY } from "../../ui/controls.tsx";
import { modelText } from "./graphText.ts";
import { modelTier } from "./modelTier.ts";

interface Props {
  provider: "codex" | "claude";
  model: string;
  /** 실행: `dontAskAgain` says "다음부터 안 보기" was checked. 취소: nothing runs. */
  onDone: (run: { dontAskAgain: boolean } | undefined) => void;
}

/**
 * The first-use Deep warning (ADR-0033): the LLM call spends the person's subscription,
 * so it never runs without a click. A native dialog — focus moves in, Esc cancels,
 * and the focus goes back to the Deep button when it closes.
 */
export function DeepConfirm({ provider, model, onDone }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  const [dontAskAgain, setDontAskAgain] = useState(false);
  const name = provider === "codex" ? "Codex" : "Claude";
  const tier = modelTier(model);

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.showModal();
    // It spends the person's quota: Enter on the first focus must not run it.
    cancelRef.current?.focus();
    return () => {
      dialog.current?.close();
      previous?.focus();
    };
  }, []);

  return (
    <dialog
      ref={dialog}
      aria-labelledby={titleId}
      onCancel={(event) => {
        event.preventDefault();
        onDone(undefined);
      }}
      className="m-auto w-[min(28rem,92vw)] rounded-xl border bg-card p-5 text-card-foreground shadow-lg backdrop:bg-black/50"
    >
      <header className="flex items-start gap-2.5">
        <Sparkles className="mt-0.5 size-5 shrink-0 text-agent-working" aria-hidden="true" />
        <h2 id={titleId} className="text-base font-semibold">
          Deep 실행 전 확인
        </h2>
      </header>
      <p className="mt-3 text-sm leading-relaxed">
        Deep은 로그인된 {name}의 <span className="font-mono text-[13px]">{modelText(model)}</span>로 영역 이름과 요약을 만듭니다. LLM 사용량이 차감됩니다. 상위
        모델일수록 차감량이 늘어납니다.
      </p>
      <p className="mt-2 text-xs text-muted-foreground">
        이 모델의 차감 등급: <span className={tier === "높음" ? "font-medium text-destructive" : tier === "낮음" ? "text-status-success" : ""}>{tier}</span>
        {model ? "" : " (기본 모델 기준)"}
      </p>
      <label className="mt-4 flex items-center gap-2 text-xs">
        <input type="checkbox" checked={dontAskAgain} onChange={(e) => setDontAskAgain(e.target.checked)} />
        다음부터 안 보기
      </label>
      <footer className="mt-5 flex items-center justify-end gap-2">
        <button ref={cancelRef} type="button" className={BUTTON_SECONDARY} onClick={() => onDone(undefined)}>
          취소
        </button>
        <button type="button" className={BUTTON_PRIMARY} onClick={() => onDone({ dontAskAgain })}>
          실행
        </button>
      </footer>
    </dialog>
  );
}
