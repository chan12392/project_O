import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { leadItems, type LeadModel, type LeadProvider, type LeadShow } from "../../../src/core/lead.ts";
import type { Backend, LeadDispatched } from "../platform/backend/index.ts";

const POLL_MS = 2_000;

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
 * Answers that land after the project changed are dropped.
 */
export function useLead(api: Backend, project: string | undefined, enabled: boolean) {
  const [show, setShow] = useState<LeadShow>();
  const [failed, setFailed] = useState<string>();
  const [sending, setSending] = useState(false);
  const wanted = useRef(project);
  wanted.current = project;
  const last = useRef("");

  const refresh = useCallback(async () => {
    if (!enabled) return;
    const asked = project;
    try {
      const next = await api.leadShow(asked);
      if (wanted.current !== asked) return;
      const text = JSON.stringify(next);
      if (text !== last.current) {
        last.current = text;
        setShow(next);
      }
      setFailed(undefined);
    } catch (e) {
      if (wanted.current === asked) setFailed(e instanceof Error ? e.message : String(e));
    }
  }, [api, project, enabled]);

  useEffect(() => {
    if (!enabled) return;
    last.current = "";
    setShow(undefined);
    let timer: number | undefined;
    let stopped = false;
    const tick = async () => {
      await refresh();
      if (!stopped && !document.hidden) timer = window.setTimeout(tick, POLL_MS);
    };
    const onVisibility = () => {
      window.clearTimeout(timer);
      if (!document.hidden) void tick();
    };
    void tick();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      stopped = true;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [refresh, enabled]);

  const thread = show?.thread && (!project || show.thread.project === project || sameFolder(show.thread.project, project)) ? show.thread : null;
  const items = useMemo(() => leadItems(thread?.events ?? []), [thread]);

  const send = useCallback(
    async (input: SendInput) => {
      if (!project) throw new Error("프로젝트 폴더를 먼저 정하세요");
      setSending(true);
      // The message is on record before the lead starts: show it (and the busy lead) right away.
      const early = window.setTimeout(() => void refresh(), 400);
      try {
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
    /** Projects with a lead thread, newest first; they show in the sidebar even before any run. */
    projects: show?.projects ?? [],
    busy: sending || Boolean(thread?.busy),
    sending,
    send,
    stop,
    startNew,
    dispatch,
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
