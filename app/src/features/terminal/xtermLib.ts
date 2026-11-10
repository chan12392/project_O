/**
 * @xterm/xterm and its fit addon as one shared promise (ADR-0042). TerminalView
 * starts the download when it mounts, in parallel with the first `termRead`,
 * and the stylesheet rides along in the same chunk — like the Graph tab's `three`.
 */
export interface XtermLibs {
  Terminal: typeof import("@xterm/xterm").Terminal;
  FitAddon: typeof import("@xterm/addon-fit").FitAddon;
}

let libs: Promise<XtermLibs> | undefined;

export function loadXterm(): Promise<XtermLibs> {
  libs ??= Promise.all([import("@xterm/xterm"), import("@xterm/addon-fit"), import("@xterm/xterm/css/xterm.css")]).then(([x, fit]) => ({
    Terminal: x.Terminal,
    FitAddon: fit.FitAddon,
  }));
  return libs;
}
