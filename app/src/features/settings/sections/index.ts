import { ABOUT_SECTION } from "./About.tsx";
import { ACCOUNT_SECTION } from "./Account.tsx";
import { AGENTS_SECTION } from "./Agents.tsx";
import { APPEARANCE_SECTION } from "./Appearance.tsx";
import { GENERAL_SECTION } from "./General.tsx";
import { LEAD_SECTION } from "./Lead.tsx";
import { NOTIFICATIONS_SECTION } from "./Notifications.tsx";
import { PROJECTS_SECTION } from "./Projects.tsx";
import { SHORTCUTS_SECTION } from "./Shortcuts.tsx";

/** Settings navigation: groups of sections, in order. A new section is a file here and its entry below. */
export const SETTINGS_GROUPS = [
  { title: "기본", items: [ACCOUNT_SECTION, GENERAL_SECTION, APPEARANCE_SECTION] },
  { title: "작업", items: [PROJECTS_SECTION, LEAD_SECTION, AGENTS_SECTION, NOTIFICATIONS_SECTION] },
  { title: "도움말", items: [SHORTCUTS_SECTION, ABOUT_SECTION] },
] as const;

export type SettingsSection = (typeof SETTINGS_GROUPS)[number]["items"][number]["id"];
