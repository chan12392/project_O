export interface UpdateRelease {
  tag: string;
  publishedAt: string;
  notes: string;
  asset: string;
  size: number;
}

export interface UpdateCheck {
  release: UpdateRelease | null;
  message: string;
}

export interface About {
  /** The desktop app's version; under the dev server, the `o` CLI's (`o about`). */
  version: string;
  /** The desktop app only: the O CLI it ships, versioned apart from the app. */
  cliVersion?: string;
  home: string;
  agentsHome: string;
  commit: string;
  builtAt: string;
}

/** The desktop shell's `update_status`: what Settings → 정보 says before anyone checks. */
export interface UpdateStatus {
  /** Reported by the native shell, not inferred from the browser. */
  platform?: string;
  /** Release builds check by themselves (10 s after start, then every 6 hours). */
  automatic: boolean;
  /** A usable `gh` on PATH; null where the screen cannot tell (dev server, demo). */
  gh: boolean | null;
  lastCheck: { at: string; tag: string | null; error: string | null } | null;
}

/** One line under "앱 업데이트": the gh requirement up front, and the last background check. */
export function updateStatusLine(status: UpdateStatus): string {
  if (status.gh === false) return "GitHub CLI(gh)를 찾지 못해 업데이트를 확인할 수 없습니다. gh를 설치하고 gh auth login --hostname github.com으로 로그인해 주세요.";
  if (!status.automatic) return "이 빌드는 자동으로 확인하지 않습니다. 업데이트 확인을 눌러 주세요.";
  const last = status.lastCheck;
  if (!last) return "앱 시작 10초 뒤 처음 확인합니다.";
  const at = new Date(last.at).toLocaleString("ko-KR");
  if (last.error) return `마지막 자동 확인(${at})에 실패했습니다: ${last.error}`;
  return `마지막 자동 확인: ${at} · ${last.tag ? `새 버전 ${last.tag}` : "새 버전 없음"}`;
}

/** Outside the desktop app there is nothing to report. */
export const NO_UPDATE_STATUS: UpdateStatus = { automatic: false, gh: null, lastCheck: null };

export const DESKTOP_UPDATES_ONLY ="업데이트는 데스크톱 앱에서만 확인할 수 있습니다.";
export const UNINSTALL_KEEP = `Start-Process -FilePath ((Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\project_O').UninstallString.Trim('"')) -ArgumentList '/S' -WindowStyle Hidden -Wait`;
export const UNINSTALL_PURGE = UNINSTALL_KEEP.replace("'/S'", "'/S /PURGE'");
export const UPDATE_EVENT = "project-o:update-available";
