# Surface brief — Verso desktop app

## Scope and mode

The whole native application: one window, source leaf, target leaf, the transfer between
them, batch list, conversion states, light and dark. Mode: **Operate** — the visitor came
to convert a file, and the interface must disappear into that job.

## Audience, job, action

Someone at a desk with one document (or a folder of exports) and a second format required.
Job: pick the file, see what the conversion will cost, convert, open the result. Action:
Convert. A second action matters as much: swap, which flips direction and reuses the output
as the next input.

## Constraints

- Two panels side by side; swap control between them; batch by dropping several files.
- Dark and light, keyboard reachable, AA contrast, one binary, nothing uploaded.
- Every loss is stated before conversion and again after; unsupported pairs explain why.
- A scanned PDF is refused, never exported empty.

## Chosen direction

**The darkroom / light table.** The two work areas are sheets: a negative under safelight in
dark mode, a proof on a light table in light mode. A persistent film-black top rail with
sprocket perforations is the shell in both themes. Type is Barlow (edge-label grotesque)
with tabular mono for filenames and format codes. Palette: film black, film-strip grey,
safelight amber as the action and progress, grease-pencil red for errors, sage for success.
The one signature move is the **transfer strip**: source frame, target frame, and an
exposure line that travels between them while converting; batch files become numbered
frames. Everything else is a standard desktop control. No ruled film borders standing in
for tables, no fake instrument bezels, no command prompts.

## Direction contract

THESIS: A conversion is an exposure, not a file dialog. The screen refuses the category
default (two grey cards around a blue button) by making the direction of the transfer — and
its cost — the largest object on screen.

OWN-WORLD: Film black `#141312`, film-strip grey `#1C1A18`, safelight amber `#E9A23B`,
grease-pencil red `#E0553C`, sage `#9CC5A1`; cool light-table paper `#E9EAE5` and panel
`#F7F7F3` with the same film-black rail. Barlow for language, tabular mono for data.
Hairlines are 1px; corners are 4px; perforation rows and frame numbers are the only
repeated motif.

STORY: The visitor understands within one glance which file becomes which format, what will
be lost, and what to press. After converting they see the proof, its byte size, where it
landed, and can turn the page with one control.

FIRST VIEWPORT: A 44px film-black rail (wordmark left, matrix and lights right). Beneath, a
full-width 72px transfer strip: source frame left with a 26px format code, target frame
right, a hairline route between them; sprocket perforations top and bottom. Below, two
equal leaves split by a 48px fold carrying the round swap control at mid-height. Left leaf:
drop frame or file list. Right leaf: pair brief with losses, or the proof preview. A 56px
footer: status left, exposure progress centre, Convert (amber) and Cancel right.

FORM: The grounded list's 6th of 7 (transit interchange, print proofing, bookbinding,
conservation lab, card catalog, **darkroom contact sheet**, papermaking); form is code-led,
seeded key a330f3a9. The transfer strip and its exposure line are the single move.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance.
