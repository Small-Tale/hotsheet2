# Image attachment crop design (HS2-VFBYZY)

## Decision

Keep the uploaded image immutable. Store at most one crop rectangle, in **original image
pixels**, on its attachment metadata. A new crop replaces that rectangle; it never crops
an already cropped rendition. Clearing it restores the original immediately. This follows
the draft-edit model in UX Review's annotation editor, section 6.6.

Annotations remain in the original image's normalized 0–10,000 coordinate space. The
gallery projects them into the cropped viewport while editing, clips visible strokes at
the crop edge, and hides shapes wholly outside. New or edited marks are mapped back to
the original before saving. Crop changes never rewrite those canonical coordinates or
delete hidden marks. Undo/redo restores crop changes as one local markup step, and
Restore Original makes every hidden annotation visible again without rounding drift.

## Storage and API

- Add optional `crop: { x, y, width, height }` to the attachment model and ticket wire
  shape. Values are integer pixels, snapped outward, at least 8 by 8, and bounded by the
  decoded original. Absence means the full image. The server validates each mutation
  against the immutable bytes; a full-image rectangle is stored as absence.
- A crop mutation and any annotation edits in the same markup session are committed
  together with one activity note. Existing annotation-only writes remain compatible.
- The existing attachment-by-id and attachment-by-name GET routes serve the cropped
  rendition by default. The new explicit original route serves the immutable bytes for
  Crop mode and Restore Original. The server derives renditions from the original and
  crop, with a cache key that includes the original digest and rectangle; a new crop
  therefore cannot expose stale pixels. Response content type and filename must match
  the encoded rendition. The original route is never substituted for a download.
- Crop supports still PNG, JPEG, WebP, AVIF, BMP, and ICO; animated PNG, GIF, WebP, and AVIF;
  and SVG with explicit pixel dimensions and a valid viewBox. Animated crops retain every
  displayed frame, timing, and repeat count. SVG remains vector markup, including animation
  elements. Animated AVIF crops currently require at least 16 pixels per side (HS2-QRHB5F).
  Video and formats without a matching rendition encoder remain unavailable. Decoding or
  encoding failure leaves both metadata and bytes unchanged and returns a useful error.
  ICO crops retain each source resolution, scaling the crop rectangle to each entry and
  encoding the result as 32-bit RGBA PNG within the ICO for reliable transparency.

## Readers and providers

Gallery, inline preview, attachment card, Markdown image, download, and AI image fetches
use the normal attachment URL, so they all see the cropped rendition. Ticket, CLI, MCP,
and AI metadata reads expose the crop rectangle and say explicitly that annotation
coordinates refer to the original. CLI and MCP currently expose metadata and annotation
mutation but no general attachment byte-download command; a future byte reader must use
the rendition by default and expose an explicit original option. The original never
becomes a second visible attachment and does not enter an attachment count or provider
upload. Cross-provider transfer must reject a cropped source until the destination can
preserve the rendition and restore semantics; silently uploading the original would be
misleading.

Provider capability discovery adds a crop capability. Git-backed tickets and GitHub
connections with an assets repository advertise it. For GitHub (HS2-KGC823), the
original blob stays immutable while a content-addressed rendition is uploaded to
the assets repository and the marked issue comment points to it. Crop and original-space
annotations live in the comment marker; `/original` reads the original blob. A comment
body revision rejects stale gallery saves, and a retry reuses an orphaned rendition.
GitHub comment PATCH lacks an atomic revision precondition, so strict concurrent-edit
exclusion remains in HS2-X09EJ1. Sources without compatible crop storage keep the
action unavailable.

## Geometry and verification

For an original point `(x, y)` normalized to 10,000, project through original pixel
dimensions, subtract the crop origin, then normalize by the crop size. Inverse mapping
uses the same original dimensions and crop. Rectangles use intersection for the visible
shape, insertion points outside are hidden, and freehand/arrow paths are clipped at the
viewport while their original points are retained. Shape bounds and hit testing use
the projected visible geometry. A crop touching the original edge is valid; tiny or
zero-area crops are not.

Unit tests cover the transform matrix (inside, partial, outside, boundary points,
all shapes, repeated crops, inverse round trips, and restore). Store/API tests cover
validation, unchanged originals, derived response bytes, activity, and cache keys.
Browser tests cover drawing and resizing a crop, local undo/redo, restore, annotation
visibility and editing, download pixels, and persisted reload. Visual review covers
wide and phone galleries, including keyboard and screen-reader labels.
