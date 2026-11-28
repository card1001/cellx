import type { PointRef } from "./formula";
import type { Selection } from "./grid/Grid";
import type { Pos } from "./grid/model";

export type Tab = { kind: "sheet"; index: number } | { kind: "data"; name: string };

export const tabKey = (tab: Tab): string =>
  tab.kind === "sheet" ? `s:${tab.index}` : `d:${tab.name}`;

export const START: Selection = {
  anchor: { r: 1, c: 1 },
  focus: { r: 1, c: 1 },
};

export interface Session {
  tab: Tab;
  pos: Pos;
  text: string;
  /** Text before editing, to skip writes that change nothing. */
  original: string;
  caret: number;
  /** enter: arrows leave the cell (or point); edit (F2): arrows move the caret. */
  mode: "enter" | "edit";
  origin: "cell" | "bar";
  point: PointRef | null;
}
