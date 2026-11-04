import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";

import { LEAD_VIEW, leadItems, leadView, type LeadModel, type LeadProvider } from "../../../src/core/lead.ts";
import { createPoll } from "../../../src/core/poll.ts";
import type { Backend, LeadDispatched } from "../platform/backend/index.ts";

const POLL_MS = 2_000;
const HIDDEN_POLL_MS = 15_000;

export interface SendInput {
  provider: LeadProvider;
  model?: string;
  effort?: string;
  text: string;
  attachments: string[];
  clean: boolean;
}

/**
 * A project's lead thread (ADR-0008): `o lead show` polled while the window is visible,
 * so a turn sent from the CLI or another window shows up too, and the actions on it.
 * Answers that land after the project changed, or after a newer answer, are dropped
 * (`leadView`); the projects with a lead thread are kept apart from the thread, so a
 * switch to one of them does not lose the list it was found in.
 */
export function useLead(api: Backend, project: string | undefined, enabled: boolean) {
  const [view, dispatchView] = useReducer(leadView, LEAD_VIEW);
  const [sending, setSending] = useState(false);
  const seq = useRef(0);
  const busyPoll = useRef(false);
  const noteWrite = useRef<Promise<void> | null>(null);

  const refresh = useCallback(async () => {
    if (!enabled) return;
    const asked = project;
    const mine = ++seq.current;
    try {
      dispatchView({ type: "answer", project: asked, seq: mine, show: await api.leadShow(asked) });
    } catch (e) {
      dispatchView({ type: "failed", project: asked, seq: mine, error: e instanceof Error ? e.message : String(e) });
    }
  }, [api, project, enabled]);

  useEffect(() => {
    if (!enabled) return;
    dispatchView({ type: "project", project });
    // One loop: a check still out when the window is shown again is not doubled.
    const poll = createPoll({
      run: refresh,
      delay: POLL_MS,
      // Hidden (minimized, in the tray) the thread is still read, slowly, so a question it asks can alert.
      hidden: () => false,
      setTimer: (fn, ms) => window.setTimeout(fn, document.hidden ? HIDDEN_POLL_MS : busyPoll.current ? 600 : ms),
      clearTimer: (id) => window.clearTimeout(id),
    });
    const onVisibility = () => poll.wake();
    poll.wake();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      poll.stop();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [refresh, enabled, project]);

  const show = view.project === project ? view.show : undefined;
  const failed = view.project === project ? view.failed : undefined;
  const thread = show?.thread && (!project || show.thread.project === project || sameFolder(show.thread.project, project)) ? show.thread : null;
  busyPoll.current = sending || Boolean(thread?.busy);
  const items = useMemo(() => leadItems(thread?.events ?? []), [thread]);

  const send = useCallback(
    async (input: SendInput) => {
      if (!project) throw new Error("프로젝트 폴더를 먼저 정하세요");
      setSending(true);
      // The message is on record before the lead starts: show it (and the busy lead) right away.
      const early = window.setTimeout(() => void refresh(), 400);
      try {
        if (noteWrite.current) await noteWrite.current;
        await api.leadSend({ project, ...input });
      } finally {
        window.clearTimeout(early);
        setSending(false);
        await refresh();
      }
    },
    [api, project, refresh],
  );

  const stop = useCallback(async () => {
    await api.leadStop(project);
    await refresh();
  }, [api, project, refresh]);

  const stopAll = useCallback(async () => {
    if (!project) return;
    await api.leadStopAll(project);
    await refresh();
  }, [api, project, refresh]);

  const startNew = useCallback(async () => {
    if (!project) return;
    await api.leadNew(project);
    await refresh();
  }, [api, project, refresh]);

  const dispatch = useCallback(
    async (planId: string, clean: boolean): Promise<LeadDispatched> => {
      try {
        return await api.leadDispatch(planId, clean);
      } finally {
        await refresh();
      }
    },
    [api, refresh],
  );

  return {
    loaded: show !== undefined,
    failed,
    thread,
    items,
    /** Projects with a lead thread, newest first; they show in the sidebar even before any run. Kept across project switches. */
    projects: view.projects,
    busy: sending || Boolean(thread?.busy),
    sending,
    /** Read the thread again now ("다시 시도"). */
    refresh,
    send,
    stop,
    stopAll,
    startNew,
    dispatch,
    changeNote: async (threadId: string, change: import("../../../src/core/lead.ts").DesignNoteChange) => {
      const write = api.leadNote(threadId, change);
      noteWrite.current = write;
      try { await write; } finally { if (noteWrite.current === write) noteWrite.current = null; }
      await refresh();
    },
  };
}

/**
 * The models a lead of this agent may name (`o lead models`, cached by the shell). `failed`
 * says why there is no list (the CLI could not list them); the pickers then offer typing an id.
 */
export function useLeadModels(api: Backend, provider: LeadProvider | undefined) {
  const [state, setState] = useState<{ provider?: LeadProvider; models: LeadModel[]; failed?: string }>({ models: [] });
  useEffect(() => {
    if (!provider) return;
    let current = true;
    api.leadModels(provider).then(
      (r) => current && setState({ provider, models: r.models }),
      (e) => current && setState({ provider, models: [], failed: e instanceof Error ? e.message : String(e) }),
    );
    return () => {
      current = false;
    };
  }, [api, provider]);
  // Until this agent's answer arrives, the last agent's list does not show under it.
  const ready = state.provider === provider;
  return { loading: Boolean(provider) && !ready, models: ready ? state.models : [], failed: ready ? state.failed : undefined };
}

/** Folders compare as O compares them: any slash, no trailing one, any case on Windows. */
export function sameFolder(a: string, b: string): boolean {
  const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
  return norm(a) === norm(b);
}

export type LeadState = ReturnType<typeof useLead>;
