# Color Filter for Random & List APIs

**Date**: 2026-06-06
**Status**: Draft

## Overview

Add color-based filtering to `GET /` (random image) and `GET /list` (paginated list) endpoints, using the same LAB color distance computation already implemented in `GET /color/search`.

## Motivation

The existing `/color/search` endpoint provides dedicated color-based search, but the `/` random and `/list` endpoints lack color filtering. Users want to browse images filtered by color while retaining random selection and pagination semantics.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Color mode | Both `primary` and `palette`, via `mode` parameter | Consistent with `/color/search` |
| Random endpoint behavior | Filter by color distance, then `ORDER BY RANDOM()` from the candidate pool | Preserves "random" semantics while narrowing by color |
| List endpoint sorting | Add `sort_by=distance` as a new sort option | Caller can combine color filter with any sort method |
| `max_dist` default | 2500 (ΔE ≈ 50 in LAB squared distance) | Wide enough for semantic color search (e.g. "warm tones"), narrow enough to avoid "everything matches" |
| Approach | Inline color filtering in existing query functions (方案 A) | Minimal change, follows existing pattern of appending WHERE conditions |

## API Changes

### `GET /` — New Query Parameters

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `r`, `g`, `b` | `u8` | — | RGB input (all three required together) |
| `l`, `a`, `b_lab` | `f64` | — | LAB direct input (all three required together) |
| `mode` | `string` | `"primary"` | `"primary"` or `"palette"` |
| `max_dist` | `f64` | `2500` | LAB squared Euclidean distance cutoff |

Input validation: must provide either complete `(r,g,b)` or complete `(l,a,b_lab)`. If neither is complete, color filtering is silently ignored (backward compatible).

### `GET /list` — New Query Parameters

Same color parameters as above, plus:

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `sort_by` | `string` | `"id"` | Now also accepts `"distance"` |

When `sort_by=distance` and color parameters are provided, results are sorted by ascending LAB distance.

### Behavior When Color Params Are Present

1. Convert input to LAB (RGB → LAB via `rgb_to_lab()` if RGB input)
2. Apply bounding box pre-filter in SQL (BETWEEN target ± sqrt(max_dist) per channel)
3. Compute exact squared Euclidean distance in Rust, discard results exceeding `max_dist`
4. `GET /`: pick one random image from filtered candidates
5. `GET /list`: sort by distance (if `sort_by=distance`) or by requested sort, paginate

## Implementation Plan

### File: `crates/randimg-core/src/handlers/image.rs`

1. Add color fields to `RandomQuery` and `ListQuery` structs
2. Add `"distance"` to `allowed_sorts` in `list_images` handler
3. Parse color params in `random_image` and `list_images` handlers, convert to LAB, pass to query functions

### File: `crates/randimg-core/src/db/query/image.rs`

4. Add color parameters to `random_image()` function signature
5. Add color parameters to `list_images()` function signature
6. Implement inline color filtering:
   - **Primary mode**: bounding box on `images.primary_l/a/b` → Rust distance filter
   - **Palette mode**: bounding box on `image_color_palette.lab_l/a/b` → Rust min-distance per image
7. For `list_images`: when `sort_by == "distance"`, sort by computed distance (similar to existing popularity sort pattern)
8. For `random_image`: apply color filter before `ORDER BY RANDOM()`

### Default max_dist

Define constant `const DEFAULT_MAX_DIST: f64 = 2500.0;` in `image.rs` (query module).

### No Changes To

- Route registration
- `main.rs` / `lib.rs`
- Other handlers or modules
- `GET /color/search` (unchanged)

## Edge Cases

- **Color params incomplete**: Ignore color filtering, behave as before
- **Empty candidate pool after filtering**: Return `404 No image found`
- **Palette mode with no palette data**: Image excluded from results (no LAB data to compare)
- **Large candidate pool in list**: Bounding box pre-filter limits DB result set; Rust post-filter is O(n) on the surviving rows
