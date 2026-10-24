# Verso — plan and conversion matrix

## 10-line plan

1. Scaffold a Cargo workspace: `core` (pure conversion, no UI) and `app` (eframe), plus release profile with `lto`/`codegen-units = 1`.
2. Define the intermediate document model (`Block`/`Inline`), `Format`, `Options`, typed `ConvertError`, and the `Converter` trait.
3. Build the central registry: one `IrConverter` instance per supported pair, plus a static conversion matrix carrying support level and loss notes.
4. Stage 1 — readers/writers for TXT, MD, HTML; normalized round-trip tests.
5. Stage 2 — PDF: `pdf-extract` for reading (scanned-page detection), a hand-written PDF writer with Base-14 Helvetica metrics for wrapping, headings, lists and tables.
6. Stage 3 — DOCX: `zip` + `roxmltree` reader (paragraphs, styles, lists, tables, images), hand-written WordprocessingML writer.
7. Stage 4 — RTF, CSV (delimiter sniffing, table mapping), JSON (document-model export, array-of-objects for tables).
8. Background worker: `rayon` batch, channel progress events, cancel flag, collision-safe output paths.
9. UI: two leaves with the fold between them, format combos, drag-and-drop, live structural preview, loss warnings, colophon, batch list, light/dark.
10. Verify: unit and round-trip tests, debug + release build, README, DESIGN.md, known limitations.

## Conversion matrix

Legend: **●** native (structure kept) · **◐** supported with a stated loss · **○** unsupported.

| from \ to | TXT | MD  | HTML | DOCX | PDF | RTF | JSON | CSV |
|-----------|-----|-----|------|------|-----|-----|------|-----|
| **TXT**   | ●   | ●   | ●    | ●    | ●   | ●   | ●    | ◐   |
| **MD**    | ●   | ●   | ●    | ●    | ◐   | ◐   | ●    | ◐   |
| **HTML**  | ●   | ●   | ●    | ●    | ◐   | ◐   | ●    | ◐   |
| **DOCX**  | ●   | ●   | ●    | ●    | ◐   | ◐   | ●    | ◐   |
| **PDF**   | ◐   | ◐   | ◐    | ◐    | ◐   | ◐   | ◐    | ○   |
| **RTF**   | ●   | ◐   | ◐    | ◐    | ◐   | ●   | ●    | ◐   |
| **JSON**  | ●   | ●   | ●    | ●    | ◐   | ◐   | ●    | ◐   |
| **CSV**   | ●   | ●   | ●    | ●    | ●   | ●   | ●    | ●   |

### Why the marked cells are marked

- **PDF as source (all text targets)** — a PDF has positioned glyphs, not headings, lists or
  tables. Verso recovers reading-order paragraphs and says so. A PDF with no text layer
  (a scan) is refused with `ScannedPdf`; it is never exported as an empty document.
- **PDF → CSV** — table structure cannot be recovered reliably, so this pair is unsupported
  and the UI disables Convert and the swap with an explanation.
- **→ PDF** — output is reflowed; original page layout is not reproduced. Images are not
  embedded (reported when the source actually has images). Non-Windows-1252 characters
  are replaced.
- **→ RTF** — tables are flattened to tab-separated lines; RTF carries presentation, not
  document structure, so a round trip back to DOCX/HTML loses heading semantics.
- **RTF as source** — text and emphasis survive; headings, lists and tables do not.
- **→ CSV** — the first table is exported. A document with no table falls back to a
  single-column export and warns.
- **JSON** — documents export as a self-describing document model; a lone table exports as
  an array of objects (and reads back as a table). Other JSON is treated as a code block.

## Assumptions (recorded instead of asked)

- The brief asks for the plan and matrix before code; no interview was requested, so the
  audience in PRODUCT.md is inferred and labelled.
- Windows is the development host; the project stays cross-platform (no OS-specific APIs
  except opening a folder/file, implemented per-OS).
- Desktop app, not a Tauri/webview app: egui keeps one process, one binary, and no web
  runtime, which matters for the "nothing leaves the machine" promise.
- Images live in the IR and survive HTML, Markdown and JSON; PDF, DOCX and RTF output
  report them as placeholders rather than dropping them silently.
