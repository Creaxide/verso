# Verso — design

## Direction

**The darkroom / light table.** A conversion is an exposure: a source frame, a target
frame, and an exposure line that travels between them. The two work areas are sheets — a
negative under safelight in dark mode, a proof on a light table in light mode. The film
rail stays black in both themes, so the app keeps one identity while the working light
changes.

The design refuses the category default (two grey cards around a blue button) by making
the direction of the transfer, and its cost, the largest object on screen.

## World

| Token | Dark | Light |
|-------|------|-------|
| Film / rail | `#141312` | `#141312` |
| Strip | `#1C1A18` | `#1C1A18` |
| Background | `#100F0E` | `#E9EAE5` |
| Leaf | `#1A1816` | `#F7F7F3` |
| Leaf alt / sheet | `#201D1A` | `#EFEEE8` |
| Text | `#EDE9E2` | `#1B1A17` |
| Muted text | `#9A9388` | `#6A655B` |
| Hairline | `#302B26` | `#D5D1C5` |
| Safelight amber | `#E9A23B` | `#C97E19` |
| Amber ink (small text) | `#E9A23B` | `#8C5509` |
| Grease-pencil red | `#E0553C` | `#B83A25` |
| Sage | `#9CC5A1` | `#4F7D58` |
| Sage ink (small text) | `#9CC5A1` | `#3F6B49` |

Hairlines are 1px, corners 4px (2px on matrix cells). Perforation rows and numbered frames
are the only repeated decoration. There are no ruled film borders standing in for tables,
no fake instrument bezels, and no command prompts.

Type: **Barlow** for language, Barlow Semi Condensed for the rail and eyebrow labels, and
tabular mono for filenames, format codes and measurements. `assets/fonts/` carries the
regular, medium, semibold, bold and two condensed faces plus the OFL license.

## Layout

- **44px rail** — wordmark left; `LOCAL ONLY`, `FORMAT MATRIX`, and the appearance control
  right. The rail is black in both themes.
- **72px transfer strip** — the signature element. Source frame left, target frame right,
  a hairline route and a support pill between them. Sprocket perforations run along the top
  and bottom. Format codes are 26px; filenames sit beneath the hairline.
- **Two equal leaves** — source and target sheets, separated by a **48px fold** carrying the
  round swap control at mid-height. Dotted guides tie the orb to both sheets.
- **56px footer** — status and technical hint left, an exposure meter centre, `OPEN FOLDER`,
  `CANCEL` and the amber `CONVERT` right.

The target leaf is the same surface in three modes: **brief** (what this pair costs),
**proof** (what was written, with a preview), and batch proof.

## Signature moves

1. **The transfer strip.** It is the only place the direction is drawn as motion: the
   format codes, the route, the support pill, and an exposure line that travels while a
   conversion runs. Batches turn the strip into a count.
2. **The fold and the swap orb.** One round control turns the proof back into the source.
   Enabled it is amber; when the reverse pair is impossible it is red-ringed with a slash
   and a tooltip that names the pair. When nothing has been converted it stays quiet.
3. **The proof.** The target leaf reports the written file, its size, warnings and a
   read-back preview, plus the folder it landed in. A warning block sits between the facts
   and the preview, not in a modal.

## States reviewed

- Empty: no source, target brief invites a drop; the strip reads `awaiting a source`.
- Source loading and source loaded: structure counts, preview, format override.
- Pair brief: clean, lossy and impossible variants, with the specific losses listed.
- Converting: indeterminate exposure for one file, determinate progress for a batch.
- Proof: written file, size, warnings, preview, open file/folder.
- Batch: numbered frames, per-file status, batch proof with done/failed/skipped/written,
  aggregate folder, footer pair summary; Convert disables when every file is written.
- Swap: enabled, blocked by an impossible reverse pair, and quiet before the first proof.
- Matrix: 8×8, current row locked to the source, hover notes, click to set the target.
- Appearance: film dark, light table, and auto following the system.

## Finish review

**Method.** Built both profiles locally; exercised every state with synthetic fixtures
(two text documents, a Markdown contact sheet, an HTML page, a DOCX, a PDF, an RTF, a
JSON array and a CSV table). Captures were taken with `PrintWindow` from the running debug
build at a 1196×819 window (1180×780 client plus border and title bar). The fixtures are
invented for development; no customer data, testimonial, price or benchmark appears in the
product.

**Findings and repairs made in this pass**

- Batch footer repeated the status line; it now carries the source pair, size and
  failures/warnings instead.
- `CONVERT` stayed enabled after a completed batch; it now disables with the reason
  “Every file in this batch is written…”, while failed and skipped files remain retryable.
- Single-page PDFs carried a stray `1 / 1` footer into read-back; page numbering now starts
  only when there is more than one page, and a test pins it.
- JSON tables alphabetized their columns because `serde_json` used a sorted map; key order
  is now preserved, with a regression test.
- Small amber and sage labels failed AA against the light leaf; the palette gained
  `amber_ink` and `sage_ink` (dark ochre `#8C5509`, deep sage `#3F6B49`) for text while the
  brighter accents stay on fills, lines and large display type.
- Batch rows now use their recorded output path and elapsed time in the row tooltip instead
  of carrying dead fields.
- The batch/list scroll areas, dropdown height (all eight formats), strip spacing and
  font fallback for arrows, ellipses and em dashes were corrected earlier in the build.

**Checklist**

| Axis | Verdict |
|------|---------|
| One-glance story | Pass — pair, support and action are legible before reading any body text |
| Hierarchy | Pass — strip > leaf headline > brief/proof > footer detail |
| State coverage | Pass — empty, load, brief, busy, proof, batch, swap, matrix, both themes |
| Honesty | Pass — lossy notes before and after; scanned PDF refused; images and encoding reported |
| Consistency | Pass — same strip, fold, footer and controls in every state |
| Accessibility | Pass for keyboard paths, visible focus, non-color state cues and AA text contrast; screen-reader parity is bounded by egui |
| Motion | Pass — one signature move; progress is seekable state, not decoration |
| Performance | Pass — conversion off the UI thread, parallel batch, 70ms repaint while busy |

**Residual gaps (documented, not hidden)**

- PDF input loses heading/list/table semantics by nature; the UI says so before and after.
- PDF output reflows and uses Base-14 fonts + Windows-1252; images and non-Win-1252 text are
  reported, never silently changed.
- DOCX/RTF/TXT output does not embed images; placeholders and warnings are written instead.
- A single in-memory conversion cannot be interrupted mid-parse; cancellation is between
  batch files. The footer says “the current file finishes first”.
- Screen-reader coverage depends on egui's AccessKit integration and has not been tested
  with a real assistive-technology user.

## Verdict

**Ship it as 0.1.** The interface does the job it was built for: the direction of the
transfer is the largest thing on screen, the cost is stated before the click, the proof is
shown after it, and the swap turns the result into the next source without a second file
dialog. The remaining gaps are properties of the formats and of egui, not unfinished
surface.

## Raster provenance

No raster assets ship with Verso. The binary contains the six Barlow faces in
`assets/fonts/` (SIL Open Font License, `OFL.txt`); every other mark — sprockets, the
aperture, the swap orb, arrows, route and progress — is drawn with egui's vector painter.

The captures used during the finish review are review artifacts, not product assets:

| Capture | State | Provenance |
|---------|-------|------------|
| `verso-window.png` | source loaded | `PrintWindow` of the local debug build, synthetic fixture |
| `verso-batch-done2.png` | batch proof | same build and method, three synthetic sources |
| `verso-light-final.png` | light theme, pair brief | same build, then the rail's appearance control |
| `verso-light-csvpdf.png` | CSV → PDF proof, blocked swap | same build with the then-current debug hook |
| `verso-matrix.png` | matrix over a DOCX source | same build and method |
| `verso-swap-after.png` | swap completed, PDF source | same build and method |
| `verso-json2.png` | JSON source, key order preserved | same build and method |

All captures are 1196×819 window images (1180×780 client) taken on Windows at 100%
display scaling; the fixtures in `%TEMP%\verso-samples` were generated for this project and
contain invented darkroom notes only.
