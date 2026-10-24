# Product

<!-- impeccable:product-schema 1 -->

## Platform

adaptive

Native desktop (Windows, macOS, Linux). One product, one visual language; it adapts to the
host's light or dark setting. Not a web app, not a mobile app.

## Stack

Rust 2021, `egui`/`eframe` for native UI, a `core` library crate with no UI code, and an
`app` binary crate. Single self-contained executable; assets are compiled in.

## Users

*(Inferred from the brief — the brief specifies the job but names no audience.)*
People who already have a document and need it in another format: a researcher sending a
DOCX to a journal that wants PDF, a support engineer turning a CSV export into a readable
HTML page, a writer moving a Markdown draft into Word. They are at a desk, usually under
office lighting, often with two or three files open, and they convert in short bursts
rather than living in the app.

## Product Purpose

Convert a document between TXT, MD, HTML, DOCX, PDF, RTF, JSON, and CSV without leaving
the desktop, without uploading anything, and without guessing which conversions are safe.
Success is one click from "I have this file" to "I have that file, and I know what changed."

## Positioning

The swap is the product. Every other converter makes you re-pick the input after a
conversion; Verso turns the page: one control flips the direction and, when a conversion
has just finished, the output becomes the new input. The conversion matrix is shown, not
hidden — lossy pairs are warned about before you convert and explained after.

## Operating Context

- A conversion is a short, frequent errand: drop, choose target, convert, open folder.
- Batch drops are common when a folder of exports arrives at once.
- Files can be private (contracts, manuscripts, medical notes); nothing leaves the machine.
- Output lives next to the input. The input file is never modified or overwritten.
- PDF is usually the end of a chain, and the point where layout is lost.

## Capabilities and Constraints

- Formats: TXT, MD, HTML, DOCX, PDF, RTF, JSON, CSV as both source and target, except the
  pairs the matrix marks unsupported (PDF → CSV).
- Common intermediate document model: headings, paragraphs, inline emphasis, links,
  lists, tables, quotes, code blocks, images, rules.
- Scanned PDFs without a text layer are refused with an explanation, never written as an
  empty file.
- Conversions run off the UI thread; batch conversions run in parallel; progress is
  reported per file and is cancellable between files.
- Output naming: `<stem>.<target>` next to the source, then `_1`, `_2`, … on collision.
- A 64 MB per-file default limit keeps parsing predictable; the limit and the in-memory
  parse are documented limitations, not hidden behavior.
- Text is Windows-1252 for PDF/RTF output; characters outside it are replaced and reported.
- No `unwrap()` in non-test code. Every user-facing failure has a plain-language message.

## Brand Commitments

The user named the app only as "a desktop file-conversion app" and pinned the layout
(two panels, swap between them). The name **Verso**, the voice, and the visual world are
design decisions recorded in DESIGN.md; they are not user commitments.

## Evidence on Hand

No real customer data, testimonials, prices, or benchmarks exist and none may be invented.
The conversion samples used in development are synthetic and are labelled as such.

## Product Principles

1. Show the trade before the click; never fail silently.
2. The output is never a surprise: name, location, and losses are visible.
3. One motion per intention; the swap is the single memorable move.
4. Local by default: no network in the conversion path.
5. A conversion is done when the file opens, not when the spinner stops.

## Accessibility & Inclusion

Desktop keyboard operation for every action, visible focus, AA contrast in both themes,
and no meaning carried by color alone (icons and text accompany state colors). Screen
reader support is bounded by egui's AccessKit integration; this is recorded as a known
limitation.
