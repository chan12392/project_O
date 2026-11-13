/** The force layout off the main thread (ADR-0041): one request in, one array of positions out. */
import { layout, type LayoutInput } from "./graphLayout.ts";

export interface LayoutRequest extends LayoutInput {
  job: number;
}

export interface LayoutAnswer {
  job: number;
  positions: Float32Array;
}

const worker = self as unknown as {
  onmessage: ((event: MessageEvent<LayoutRequest>) => void) | null;
  postMessage(message: LayoutAnswer, transfer: Transferable[]): void;
};

worker.onmessage = ({ data }) => {
  const positions = layout(data);
  worker.postMessage({ job: data.job, positions }, [positions.buffer]);
};
