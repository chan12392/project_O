/**
 * The 3D libraries behind the graph canvas as one shared promise (ADR-0033). GraphScreen
 * starts the download when it mounts — in parallel with the data request — so the first
 * paint does not wait for these chunks after the view is already in hand.
 */
export interface GraphLibs {
  ForceGraph3D: typeof import("3d-force-graph").default;
  /** `three` ships no type declarations; Graph3D declares the slice it uses. */
  three: unknown;
}

let libs: Promise<GraphLibs> | undefined;

export function loadGraphLibs(): Promise<GraphLibs> {
  libs ??= Promise.all([
    import("3d-force-graph"),
    // @ts-expect-error three ships no type declarations; `GraphLibs.three` stays `unknown`.
    import("three"),
  ]).then(([mod, three]) => ({ ForceGraph3D: mod.default, three: three as unknown }));
  return libs;
}
