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
  version: string;
  home: string;
  agentsHome: string;
  commit: string;
  builtAt: string;
}

export const DESKTOP_UPDATES_ONLY = "업데이트는 데스크톱 앱에서만 확인할 수 있습니다.";
export const UNINSTALL_KEEP = `Start-Process -FilePath ((Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\project_O').UninstallString.Trim('"')) -ArgumentList '/S' -WindowStyle Hidden -Wait`;
export const UNINSTALL_PURGE = UNINSTALL_KEEP.replace("'/S'", "'/S /PURGE'");
export const UPDATE_EVENT = "project-o:update-available";
