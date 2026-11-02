/**
 * One polling loop: `run` again `delay` after the last check ended, never two checks at once,
 * paused while the window is hidden. `wake` (the window shown again, or a caller wanting fresh
 * data) checks now unless a check is already out; that one schedules the next when it ends.
 * Timers are injected so the loop can be tested without a browser.
 */
export interface PollOptions {
  run: () => Promise<unknown>;
  delay: number;
  hidden: () => boolean;
  setTimer: (fn: () => void, ms: number) => number;
  clearTimer: (id: number) => void;
}

export function createPoll({ run, delay, hidden, setTimer, clearTimer }: PollOptions) {
  let timer: number | undefined;
  let inflight = false;
  let stopped = false;
  const clear = () => {
    if (timer !== undefined) clearTimer(timer);
    timer = undefined;
  };
  const schedule = () => {
    clear();
    if (!stopped && !hidden()) timer = setTimer(tick, delay);
  };
  async function tick() {
    if (stopped || inflight) return;
    clear();
    inflight = true;
    try {
      await run();
    } catch {
      // `run` reports its own failures; the loop goes on.
    } finally {
      inflight = false;
    }
    schedule();
  }
  return {
    /** Check now (unless one is out or the window is hidden). */
    wake: () => {
      if (hidden()) clear();
      else void tick();
    },
    stop: () => {
      stopped = true;
      clear();
    },
  };
}
