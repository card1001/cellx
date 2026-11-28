import type {
  Chart,
  NameView,
  PivotDef,
  Shape,
  Slicer,
  SparkKind,
  SparklineGroup,
} from "./api";
import type { Range } from "./grid/model";
import type { ImportChoice } from "./components/DataDialogs";

export type Dialog =
  | { kind: "prompt"; title: string; label: string; value: string; resolve(v: string | null): void }
  | { kind: "format" }
  | { kind: "pasteSpecial" }
  | { kind: "highlight"; cf: import("./cf").HighlightKind }
  | { kind: "cfManager" }
  | { kind: "pivot"; def: PivotDef }
  | { kind: "chart"; chart: Chart }
  | { kind: "sort"; range: Range; header: boolean }
  | { kind: "textToColumns"; range: Range; samples: string[] }
  | { kind: "duplicates"; range: Range; header: boolean }
  | { kind: "validation"; current: import("./api").Validation | null }
  | { kind: "names"; list: NameView[]; startNew: boolean }
  | { kind: "hyperlink"; text: string; current: { target: string; internal: boolean } | null }
  | { kind: "goalSeek" }
  | { kind: "evaluate"; sheet: number; row: number; col: number }
  | { kind: "formulaGraph" }
  | { kind: "shapeFormat"; shape: Shape }
  | { kind: "sparkCreate"; spark: SparkKind; data: string; location: string }
  | { kind: "sparkStyle"; group: SparklineGroup }
  | { kind: "slicerInsert"; table: string; columns: string[] }
  | { kind: "dataTable"; range: Range }
  | { kind: "bookProtect" }
  | { kind: "bookUnprotect" }
  | { kind: "customViews" }
  | { kind: "scenarios"; selection: string }
  | { kind: "slicerSettings"; slicer: Slicer }
  | { kind: "print" }
  | { kind: "subtotal"; range: Range }
  | { kind: "createTable"; range: Range; header: boolean; style: string }
  | { kind: "importData"; path: string; suggested: string; csv: boolean; spreadsheet: boolean; resolve(v: ImportChoice | null): void }
  | { kind: "exportEncoding"; resolve(v: string | null | undefined): void }
  | { kind: "protect" }
  | { kind: "unprotect" }
  | null;
