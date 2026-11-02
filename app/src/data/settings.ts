import { useSyncExternalStore } from "react";

import { effortFor, isLeadProvider, type LeadProvider } from "../../../src/core/lead.ts";
import { readStored, writeStored } from "../lib/storage.ts";

/** A saved lead agent and effort, kept only while the agent can lead and takes that effort. */
function leadChoice(agent: unknown, effort: unknown): { leadAgent: LeadProvider; leadEffort: string } {
  const leadAgent = typeof agent === "string" && isLeadProvider(agent) ? agent : DEFAULTS.leadAgent;
  return { leadAgent, leadEffort: effortFor(leadAgent, typeof effort === "string" ? effort : DEFAULTS.leadEffort) };
}

/**
 * The owner's app preferences (Settings), kept in localStorage and shared by the
 * main window and the pop-out (the `storage` event carries a change across).
 * Appearance is applied to <html> here: `.dark` (orca-theme.css), the font scale
 * and `data-motion`, which app.css and the office read.
 */
export type Theme = "system" | "light" | "dark";
export type FontSize = "small" | "normal" | "large";
export type Motion = "system" | "reduce" | "full";
export type StartScreen = "last" | "onboarding";

export interface Settings {
  theme: Theme;
  fontSize: FontSize;
  motion: Motion;
  start: StartScreen;
  /** Agents run with Project O's clean test profiles, without the owner's personal harness. */
  clean: boolean;
  /** A desktop notification when a watched card finishes while the window is in the background. */
  notify: boolean;
  /** Show this PC's Claude Code sessions as runs; unset follows the source (see `sessionsShown`). */
  sessions?: boolean;
  /** Settings → 총괄: the agent and effort a new message starts with (the composer may pick others). */
  leadAgent: LeadProvider;
  leadEffort: string;
}

const KEY = "project-o.settings";
const ONBOARDED_KEY = "project-o.onboarded";
const LAST_PROJECT_KEY = "project-o.last-project";

/** The app was dark before Settings existed (index.html), so dark stays the default. */
export const DEFAULTS: Settings = {
  theme: "dark",
  fontSize: "normal",
  motion: "system",
  start: "last",
  clean: true,
  notify: false,
  leadAgent: "claude",
  leadEffort: "medium",
};

const FONT_SCALE: Record<FontSize, string> = { small: "0.9375", normal: "1", large: "1.125" };

function pick<T extends string>(value: unknown, allowed: readonly T[], fallback: T): T {
  return allowed.includes(value as T) ? (value as T) : fallback;
}

function load(): Settings {
  return readStored<Settings>(KEY, DEFAULTS, (saved) => {
    if (!saved || typeof saved !== "object") return undefined;
    const s = saved as Partial<Record<keyof Settings, unknown>>;
    return {
      theme: pick(s.theme, ["system", "light", "dark"], DEFAULTS.theme),
      fontSize: pick(s.fontSize, ["small", "normal", "large"], DEFAULTS.fontSize),
      motion: pick(s.motion, ["system", "reduce", "full"], DEFAULTS.motion),
      start: pick(s.start, ["last", "onboarding"], DEFAULTS.start),
      clean: typeof s.clean === "boolean" ? s.clean : DEFAULTS.clean,
      notify: typeof s.notify === "boolean" ? s.notify : DEFAULTS.notify,
      sessions: typeof s.sessions === "boolean" ? s.sessions : undefined,
      ...leadChoice(s.leadAgent, s.leadEffort),
    };
  });
}

const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");
const motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
const listeners = new Set<() => void>();
let current = load();

function apply() {
  const root = document.documentElement;
  const dark = current.theme === "dark" || (current.theme === "system" && darkQuery.matches);
  root.classList.toggle("dark", dark);
  root.style.colorScheme = dark ? "dark" : "light";
  root.style.setProperty("--app-font-scale", FONT_SCALE[current.fontSize]);
  root.dataset.motion = current.motion;
}

function emit() {
  apply();
  for (const listener of listeners) listener();
}

/** A system theme or motion change: a fresh snapshot, so readers of the system value re-render. */
function systemChanged() {
  current = { ...current };
  emit();
}

apply();
darkQuery.addEventListener("change", systemChanged);
motionQuery.addEventListener("change", systemChanged);
window.addEventListener("storage", (e) => {
  if (e.key !== KEY) return;
  current = load();
  emit();
});

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}

/** Claude Code sessions are shown by default on this PC's runs, and hidden in the recorded demo until asked for. */
export function sessionsShown(settings: Settings, demo: boolean): boolean {
  return settings.sessions ?? !demo;
}

/** The settings as they are now, for code that reads them once (the card announcer). */
export function settingsNow(): Settings {
  return current;
}

export function updateSettings(patch: Partial<Settings>) {
  current = { ...current, ...patch };
  writeStored(KEY, current);
  emit();
}

export function useSettings(): Settings {
  return useSyncExternalStore(subscribe, () => current);
}

/** Whether motion should be held: the owner's choice, else the system's. */
export function useReducedMotion(): boolean {
  const { motion } = useSettings();
  // The system preference is read on each render; a change re-renders through `systemChanged`.
  return motion === "reduce" || (motion === "system" && motionQuery.matches);
}

/** First launch: agent onboarding until it was finished or skipped once. */
export function onboardingDone(): boolean {
  return readStored(ONBOARDED_KEY, false, (saved) => (typeof saved === "boolean" ? saved : undefined));
}

export function markOnboardingDone() {
  writeStored(ONBOARDED_KEY, true);
}

export function lastProject(): string | undefined {
  return readStored<string | undefined>(LAST_PROJECT_KEY, undefined, (saved) => (typeof saved === "string" && saved ? saved : undefined));
}

export function rememberProject(path: string | undefined) {
  if (path) writeStored(LAST_PROJECT_KEY, path);
}
