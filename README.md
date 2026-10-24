# Verso

A local document converter for the desktop. Drop a file, choose a target, convert, and
open the result. Nothing is uploaded and the source file is never changed.

Verso is one native binary for Windows, macOS and Linux, built with Rust and
[egui/eframe](https://github.com/emilk/egui). It converts between **TXT, MD, HTML, DOCX,
PDF, RTF, JSON and CSV**, warns before a lossy pair, and reports what the target could not
keep after the conversion.

## Format support

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

PDF → CSV is the only pair Verso refuses: a PDF stores positioned glyphs, not columns, so
table structure cannot be recovered reliably. The matrix is also available in the app
(`Ctrl+M`), where every cell explains its cost on hover.

## Build and run

Requirements:

- Rust **1.85** or newer (edition 2021).
- A working linker for your platform: MSVC Build Tools or MinGW-w64 on Windows, Xcode
  Command Line Tools on macOS, and the usual X11/Wayland plus OpenGL development packages
  on Linux.

```sh
cargo run --release
```

You can pass files or folders on the command line; they are loaded at startup:

```sh
cargo run --release -- ~/Documents/report.docx
cargo run --release -- ~/exports
```

Build the binary on its own:

```sh
cargo build --release
# target/release/verso        (macOS / Linux)
# target\release\verso.exe    (Windows)
```

The release profile uses `lto = true`, `codegen-units = 1` and `strip = true`, so the
binary is self-contained; fonts and UI code are compiled in.

Run the tests:

```sh
cargo test --workspace
```

## Using Verso

| Action | How |
|--------|-----|
| Open a file | Drop it on the window, press `Ctrl+O`, or pass it on the command line |
| Batch | Drop several files, or a folder; each file becomes a numbered frame |
| Choose a target | The target leaf's format menu |
| Convert | `CONVERT` or `Ctrl+Enter` |
| Stop a batch | `CANCEL` or `Esc` — the current file finishes first |
| Swap | `Ctrl+Shift+S` or the round control in the fold |
| Matrix | `Ctrl+M` |
| Open the result | `OPEN FILE` / `OPEN FOLDER`, or `OPEN FOLDER` after a batch |
| Appearance | `AUTO` / `DARK` / `LIGHT` in the rail |

Details worth knowing:

- **Swap turns the proof into the source.** After a conversion it flips the direction and
  loads the output as the next input. It is disabled when the reverse pair does not exist
  (for example CSV → PDF → CSV), with a tooltip saying why.
- **Nothing is overwritten.** Output is written next to the source as `<stem>.<target>`,
  then `_1`, `_2`, … if that name exists.
- **Batch progress is per file.** Every row reports its own state; the run is parallel
  and stops between files when cancelled.
- **The preview is honest.** The source leaf shows the parsed structure and the target
  leaf shows the pair's losses before the conversion, then a proof preview afterwards.

## Project layout

```text
Cargo.toml               workspace, shared dependencies, release profile
core/                    conversion library — no UI, no file system writes
  src/document.rs        common intermediate model: blocks, inlines, tables, images
  src/format.rs          format enum, codes, extensions, content sniffing
  src/matrix.rs          support levels and plain-language loss notes per pair
  src/registry.rs        Converter trait, pair registry, warnings
  src/readers/*.rs       one module per format: reader, writer and tests
  src/paths.rs           collision-safe output naming
  src/error.rs           typed errors with stable codes
  src/options.rs         page size, margins, limits, JSON image budget
app/                     native UI
  src/main.rs            window setup
  src/app.rs             state machine, actions, worker wiring, shortcuts
  src/worker.rs          background load/convert, rayon batch, progress events
  src/state.rs           source, batch, completed proof, notices
  src/theme.rs           darkroom / light-table palette, Barlow fonts
  src/ui/*.rs            rail, transfer strip, leaves, fold, footer, matrix
  src/components.rs      painted controls, sprockets, route, swap orb
assets/fonts/            Barlow (SIL Open Font License)
PRODUCT.md               product brief and constraints
PLAN.md                  build plan and conversion matrix
DESIGN.md                visual direction, finish review and verdict
SURFACE-BRIEF.md         direction contract for the UI
```

## Adding a format

The intermediate model is the seam: a new format is one reader, one writer and a matrix
row, not eight pair implementations.

1. **Add the format** in `core/src/format.rs`: a variant, its `code()` (two or three
   letters), `name()`, `extension()` / `from_extension()`, add it to `Format::ALL`, and
   teach `sniff()` if extensionless files should be recognized.
2. **Implement the reader and writer** in `core/src/readers/<name>.rs`. They implement
   the two small contracts:

   ```rust
   pub trait Reader: Send + Sync {
       fn read(&self, input: &[u8], options: &Options) -> Result<Document, ConvertError>;
   }

   pub trait Writer: Send + Sync {
       fn write(&self, document: &Document, options: &Options) -> Result<Vec<u8>, ConvertError>;
   }
   ```

   Export a `pub static READER` (and `WRITER` if the format can be a target), and add the
   module to `core/src/readers/mod.rs`.
3. **Register the pair rules** in `core/src/matrix.rs` — support level (`Native`, `Lossy`,
   `Unsupported`) and loss notes — and add the reader/writer arms in
   `core/src/registry.rs` (`reader_for` / `writer_for`). `IrConverter` then covers every
   supported pair automatically. No multi-pair code is needed.
4. **Name the extension** in `app/src/app.rs` (`SUPPORTED_EXTENSIONS`) so the picker,
   drag-and-drop and folder scans accept it. The dropdowns and matrix read `Format::ALL`.
5. **Add tests** next to the reader/writer, including a round trip through the new format
   where one exists. `core/src/registry.rs` has tests that fail if a supported pair has no
   converter, so the matrix and the registry cannot drift apart.

Rule of thumb for the writer: the model is deliberately smaller than any one format. If a
feature cannot be represented, write a readable placeholder and push a warning — never
drop content silently.

## Known limitations

- **PDF input is text-first.** `pdf-extract` returns positioned text, so headings, lists
  and tables come back as paragraphs; the loss is stated in the UI. A PDF with no text layer
  (a scan) is refused with an explanation rather than exported as an empty file.
- **PDF output reflows.** Page layout, fonts and exact line breaks are not reproduced.
  The writer uses the Base-14 Helvetica/Courier fonts and Windows-1252 text; characters
  outside that encoding become `?` and a warning is added. Images are written as labelled
  placeholders and reported.
- **RTF carries presentation, not document structure.** Tables flatten to tab-separated
  lines and headings lose their semantics when read back. Images become placeholders.
- **DOCX output does not embed images** and does not reproduce page geometry; images are
  reported as omitted.
- **HTML images by URL cannot travel.** Only inline `data:` images are carried into the
  document; remote references become text so the conversion never touches the network.
- **TXT/CSV cannot carry formatting.** CSV exports the first table (or a single-column
  fallback with a warning); TXT writes images as `[image: alt]` text.
- **Input limit: 64 MB per file.** Conversion is in memory, by design; the limit and the
  in-memory trade-off are stated in `Options`.
- **Cancellation granularity.** A batch stops between files; a single conversion cannot be
  interrupted safely mid-parse and finishes before the window returns to idle.
- **Screen reader support** is bounded by egui's AccessKit integration; keyboard access
  covers every action and focus is visible, but full assistive-technology parity is not
  claimed.
- **JSON is a document model, not an arbitrary serializer.** A lone table exports as an
  array of objects; other documents use a `"format": "verso.document"` envelope; general
  JSON is preserved as a code block rather than mapped into prose.

## License

MIT. Barlow is licensed under the SIL Open Font License; see `assets/fonts/`.
