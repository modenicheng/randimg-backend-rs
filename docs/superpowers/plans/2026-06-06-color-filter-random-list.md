# Color Filter for Random & List APIs — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add color-based filtering (primary/palette, RGB/LAB input) to `GET /` and `GET /list` endpoints, reusing existing LAB distance computation from `GET /color/search`.

**Architecture:** Inline color filtering in the existing `random_image()` and `list_images()` DB query functions. Bounding box pre-filter in SQL, exact distance computation in Rust, then random selection / pagination on the filtered result set. Palette mode joins `image_color_palette` table.

**Tech Stack:** Rust Edition 2024, Axum 0.8, SeaORM 1, existing `crate::color::rgb_to_lab()`

**Spec:** `docs/superpowers/specs/2026-06-06-color-filter-random-list-design.md`

---

### Task 1: Add color query parameters to handler structs

**Files:**
- Modify: `crates/randimg-core/src/handlers/image.rs`

- [ ] **Step 1: Add color fields to `RandomQuery`**

Add to the existing `RandomQuery` struct (after `tags` field, before closing `}`):

```rust
// Color filter
pub r: Option<u8>,
pub g: Option<u8>,
pub b: Option<u8>,
pub l: Option<f64>,
pub a: Option<f64>,
pub b_lab: Option<f64>,
pub mode: Option<String>,
pub max_dist: Option<f64>,
```

- [ ] **Step 2: Add color fields to `ListQuery`**

Add to the existing `ListQuery` struct (after `tags` field, before closing `}`):

```rust
// Color filter
pub r: Option<u8>,
pub g: Option<u8>,
pub b: Option<u8>,
pub l: Option<f64>,
pub a: Option<f64>,
pub b_lab: Option<f64>,
pub mode: Option<String>,
pub max_dist: Option<f64>,
```

- [ ] **Step 3: Verify compilation**

```bash
cargo check -p randimg-core 2>&1 | head -20
```
Expected: compiles (new fields are `Option`, backward compatible).

- [ ] **Step 4: Commit**

```bash
git add crates/randimg-core/src/handlers/image.rs
git commit -m "feat: add color query params to RandomQuery and ListQuery structs"
```

---

### Task 2: Define `ColorFilterParams` type + parse color params in handler

**Files:**
- Modify: `crates/randimg-core/src/db/query/image.rs` (add type)
- Modify: `crates/randimg-core/src/handlers/image.rs:107-139`

- [ ] **Step 1: Add `DEFAULT_MAX_DIST` and `ColorFilterParams` to query module**

In `crates/randimg-core/src/db/query/image.rs`, after imports and before `exclude_deleted`:

```rust
/// Default max squared Euclidean distance in LAB space for color filtering.
/// ΔE ≈ 50 — wide enough for semantic color search.
pub const DEFAULT_MAX_DIST: f64 = 2500.0;

/// Parsed color filter parameters from API query.
pub struct ColorFilterParams {
    pub lab: [f64; 3],
    pub mode: String,
    pub max_dist: f64,
}
```

- [ ] **Step 2: Add color param parsing helper in handler**

In `crates/randimg-core/src/handlers/image.rs`, above `random_image`:

```rust
use crate::db::query::image::ColorFilterParams;

/// Parse color filter params from query, converting RGB to LAB if needed.
/// Returns ColorFilterParams or None if no valid color input.
fn parse_color_params(
    r: Option<u8>, g: Option<u8>, b: Option<u8>,
    l: Option<f64>, a: Option<f64>, b_lab: Option<f64>,
    mode: Option<String>,
    max_dist: Option<f64>,
) -> Option<ColorFilterParams> {
    use crate::db::query::image::DEFAULT_MAX_DIST;

    let lab = if let (Some(r), Some(g), Some(b)) = (r, g, b) {
        let lab = crate::color::rgb_to_lab(r, g, b);
        [lab[0] as f64, lab[1] as f64, lab[2] as f64]
    } else if let (Some(l), Some(a), Some(b_lab)) = (l, a, b_lab) {
        [l, a, b_lab]
    } else {
        return None;
    };

    let mode = mode.unwrap_or_else(|| "primary".to_string());
    if mode != "primary" && mode != "palette" {
        return None;
    }

    let max_dist = max_dist.unwrap_or(DEFAULT_MAX_DIST);

    Some(ColorFilterParams { lab, mode, max_dist })
}
```

- [ ] **Step 3: Modify `random_image` handler to parse and pass color params**

Replace the handler body (lines 107-139) with:

```rust
/// GET /  Random image
pub async fn random_image(
    State(state): State<Arc<WorkerState>>,
    Query(query): Query<RandomQuery>,
) -> Result<Response, AppError> {
    let ratio_floor = query.ratio_floor.unwrap_or(0.0).max(0.0);
    let ratio_ceil = query.ratio_ceil.unwrap_or(10.0).max(ratio_floor);
    let width_floor = query.width_floor.unwrap_or(0);
    let width_ceil = query.width_ceil.unwrap_or(i32::MAX);
    let height_floor = query.height_floor.unwrap_or(0);
    let height_ceil = query.height_ceil.unwrap_or(i32::MAX);

    let color_params = parse_color_params(
        query.r, query.g, query.b,
        query.l, query.a, query.b_lab,
        query.mode, query.max_dist,
    );

    let img = image::random_image(
        &state.db,
        ratio_floor,
        ratio_ceil,
        width_floor,
        width_ceil,
        height_floor,
        height_ceil,
        query.author.as_deref(),
        query.tags.as_deref(),
        color_params,
        &state.config,
    )
    .await
    .map_err(AppError::from)?;

    let img = img.ok_or(AppError::NotFound("No image found".into()))?;

    let format = query.format.as_deref().unwrap_or("json");
    let local = query.local.unwrap_or(false);

    format_image_response(&img, &state, format, local).await
}
```

- [ ] **Step 4: Verify compilation**

```bash
cargo check -p randimg-core 2>&1 | head -30
```
Expected: error at `image::random_image(...)` — the query function doesn't take `color_params` yet. This is expected.

- [ ] **Step 5: Commit**

```bash
git add crates/randimg-core/src/db/query/image.rs crates/randimg-core/src/handlers/image.rs
git commit -m "feat: parse color params in random_image handler"
```

---

### Task 3: Parse color params in `list_images` handler, add `distance` to allowed sorts

**Files:**
- Modify: `crates/randimg-core/src/handlers/image.rs:163-231`

- [ ] **Step 1: Add `"distance"` to `allowed_sorts`**

Change line 192 from:
```rust
    let allowed_sorts = [
        "id",
        "width",
        "height",
        "aspect_ratio",
        "source_created_at",
        "created_at",
        "popularity",
    ];
```
to:
```rust
    let allowed_sorts = [
        "id",
        "width",
        "height",
        "aspect_ratio",
        "source_created_at",
        "created_at",
        "popularity",
        "distance",
    ];
```

- [ ] **Step 2: Modify `list_images` handler to parse and pass color params**

Replace the handler body (lines 163-231) with:

```rust
/// GET /list  Paginated image list
#[axum::debug_handler]
pub async fn list_images(
    State(state): State<Arc<WorkerState>>,
    Query(query): Query<ListQuery>,
    auth: OptionalAuthUser,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let is_admin = auth.username.is_some();

    let offset = query.offset.unwrap_or(0).min(100_000);
    let limit = query.limit.unwrap_or(30).min(300);

    let desc = query
        .desc
        .as_deref()
        .map(|d| d.to_lowercase() == "true")
        .unwrap_or(true);

    let accessible = if is_admin {
        match query.accessible.as_deref() {
            Some("true") => Some(true),
            Some("false") => Some(false),
            _ => None,
        }
    } else {
        Some(true)
    };

    let sort_by = query.sort_by.as_deref().unwrap_or("id");
    let allowed_sorts = [
        "id",
        "width",
        "height",
        "aspect_ratio",
        "source_created_at",
        "created_at",
        "popularity",
        "distance",
    ];
    if !allowed_sorts.contains(&sort_by) {
        return Err(AppError::BadRequest(format!(
            "Invalid sort_by '{}'. Allowed: {}",
            sort_by,
            allowed_sorts.join(", ")
        )));
    }

    let color_params = parse_color_params(
        query.r, query.g, query.b,
        query.l, query.a, query.b_lab,
        query.mode, query.max_dist,
    );

    let result = image::list_images(
        &state.db,
        offset,
        limit,
        desc,
        sort_by,
        query.ratio_floor.unwrap_or(0.0),
        query.ratio_ceil.unwrap_or(10.0),
        query.width_floor.unwrap_or(0),
        query.width_ceil.unwrap_or(i32::MAX),
        query.height_floor.unwrap_or(0),
        query.height_ceil.unwrap_or(i32::MAX),
        query.author.as_deref(),
        accessible,
        query.tags.as_deref(),
        color_params,
        is_admin,
        &state.config,
    )
    .await
    .map_err(AppError::from)?;

    Ok(Json(result))
}
```

- [ ] **Step 3: Verify compilation**

```bash
cargo check -p randimg-core 2>&1 | head -30
```
Expected: error at `image::list_images(...)` — the query function doesn't take `color_params` yet. Expected.

- [ ] **Step 4: Commit**

```bash
git add crates/randimg-core/src/handlers/image.rs
git commit -m "feat: parse color params in list_images handler, add distance sort"
```

---

### Task 4: Add color filtering to `random_image` query function

**Files:**
- Modify: `crates/randimg-core/src/db/query/image.rs:143-216`

- [ ] **Step 1: Add distance computation helper**

After the `DEFAULT_MAX_DIST` and `ColorFilterParams` (added in Task 2), add:

```rust
/// Compute squared Euclidean distance between two LAB colors.
fn lab_sq_dist(l1: f64, a1: f64, b1: f64, l2: f64, a2: f64, b2: f64) -> f64 {
    (l1 - l2).powi(2) + (a1 - a2).powi(2) + (b1 - b2).powi(2)
}
```

- [ ] **Step 2: Modify `random_image` function signature**

Change the function signature (line 143-154) to add `color_params`:

```rust
/// Get random accessible image, optionally filtered by color
pub async fn random_image(
    db: &DatabaseConnection,
    ratio_floor: f32,
    ratio_ceil: f32,
    width_floor: i32,
    width_ceil: i32,
    height_floor: i32,
    height_ceil: i32,
    author_filter: Option<&str>,
    tags: Option<&str>,
    color_params: Option<ColorFilterParams>,
    config: &AppConfig,
) -> Result<Option<serde_json::Value>, DbErr> {
```

- [ ] **Step 3: Add color bounding box pre-filter (primary mode)**

After the existing filters (after the tags/distinct block, before `ORDER BY RANDOM()`), add:

```rust
    // Color filter: primary mode bounding box pre-filter
    if let Some(ref cp) = color_params {
        if cp.mode == "primary" {
            let radius = cp.max_dist.sqrt();
            query = query
                .filter(image::Column::PrimaryL.is_not_null())
                .filter(image::Column::PrimaryL.between(cp.lab[0] - radius, cp.lab[0] + radius))
                .filter(image::Column::PrimaryA.between(cp.lab[1] - radius, cp.lab[1] + radius))
                .filter(image::Column::PrimaryB.between(cp.lab[2] - radius, cp.lab[2] + radius));
        }
    }
```

- [ ] **Step 4: Add color post-filter and random selection**

Replace the `order_by_asc(Expr::cust("RANDOM()"))` logic (lines 208-216) with:

```rust
    // Apply color post-filtering (for both primary and palette modes)
    if let Some(ref cp) = color_params {
        if cp.mode == "primary" {
            // Primary mode: bounding box filtered some rows, exact-filter the rest
            let results = query.all(db).await?;
            let filtered: Vec<image::Model> = results
                .into_iter()
                .filter(|img| {
                    let l = img.primary_l.unwrap_or(0.0);
                    let a = img.primary_a.unwrap_or(0.0);
                    let b = img.primary_b.unwrap_or(0.0);
                    lab_sq_dist(l, a, b, cp.lab[0], cp.lab[1], cp.lab[2]) <= cp.max_dist
                })
                .collect();

            if filtered.is_empty() {
                return Ok(None);
            }

            // Random pick from filtered candidates
            use rand::seq::SliceRandom;
            let img = filtered.choose(&mut rand::thread_rng()).cloned();
            let Some(img) = img else {
                return Ok(None);
            };
            return find_by_id(db, img.id, false, config).await;
        } else {
            // Palette mode: query image_color_palette for matching colors
            use crate::db::entities::image_color_palette::{self, Entity as PaletteEntity};

            // But first, get the image IDs matching other filters from the existing query
            let candidate_images = query.all(db).await?;
            let candidate_ids: Vec<i32> = candidate_images.iter().map(|i| i.id).collect();

            if candidate_ids.is_empty() {
                return Ok(None);
            }

            let radius = cp.max_dist.sqrt();
            let palette_rows = PaletteEntity::find()
                .filter(image_color_palette::Column::ImageId.is_in(candidate_ids))
                .filter(image_color_palette::Column::LabL.between(cp.lab[0] - radius, cp.lab[0] + radius))
                .filter(image_color_palette::Column::LabA.between(cp.lab[1] - radius, cp.lab[1] + radius))
                .filter(image_color_palette::Column::LabB.between(cp.lab[2] - radius, cp.lab[2] + radius))
                .all(db)
                .await?;

            // Group by image_id, keep min distance
            let mut best_by_image: std::collections::HashMap<i32, f64> =
                std::collections::HashMap::new();
            for p in &palette_rows {
                let dist = lab_sq_dist(
                    p.lab_l, p.lab_a, p.lab_b,
                    cp.lab[0], cp.lab[1], cp.lab[2],
                );
                if dist <= cp.max_dist {
                    let entry = best_by_image.entry(p.image_id).or_insert(dist);
                    if dist < *entry {
                        *entry = dist;
                    }
                }
            }

            if best_by_image.is_empty() {
                return Ok(None);
            }

            // Random pick from palette-matched images
            let matched_ids: Vec<i32> = best_by_image.keys().cloned().collect();
            use rand::seq::SliceRandom;
            let picked_id = matched_ids.choose(&mut rand::thread_rng()).copied();

            let Some(picked_id) = picked_id else {
                return Ok(None);
            };
            return find_by_id(db, picked_id, false, config).await;
        }
    }

    // No color filter — original random selection behavior
    let img = query.order_by_asc(Expr::cust("RANDOM()")).one(db).await?;

    let Some(img) = img else {
        return Ok(None);
    };

    find_by_id(db, img.id, false, config).await
}
```

- [ ] **Step 5: Add `rand` dependency if not already present**

Check Cargo.toml for `rand`:
```bash
grep -r '"rand"' crates/randimg-core/Cargo.toml
```
If not present, add to `crates/randimg-core/Cargo.toml` under `[dependencies]`:
```toml
rand = "0.8"
```

- [ ] **Step 6: Verify compilation**

```bash
cargo check -p randimg-core 2>&1
```
Expected: compiles successfully (handler now matches query function signature).

- [ ] **Step 7: Commit**

```bash
git add crates/randimg-core/src/db/query/image.rs crates/randimg-core/Cargo.toml
git commit -m "feat: add color filtering to random_image query (primary + palette)"
```

---

### Task 5: Add color filtering to `list_images` query function

**Files:**
- Modify: `crates/randimg-core/src/db/query/image.rs:219-433`

- [ ] **Step 1: Modify `list_images` function signature**

Change the function signature (lines 219-236) to add `color_params`:

```rust
/// Paginated image list, optionally filtered by color
pub async fn list_images(
    db: &DatabaseConnection,
    offset: u64,
    limit: u64,
    desc: bool,
    sort_by: &str,
    ratio_floor: f32,
    ratio_ceil: f32,
    width_floor: i32,
    width_ceil: i32,
    height_floor: i32,
    height_ceil: i32,
    author_filter: Option<&str>,
    accessible: Option<bool>,
    tags: Option<&str>,
    color_params: Option<ColorFilterParams>,
    is_admin: bool,
    config: &AppConfig,
) -> Result<Vec<serde_json::Value>, DbErr> {
```

- [ ] **Step 2: Add color bounding box pre-filter (primary mode)**

After the existing tags filter block (before the popularity sort check, around line 292), add:

```rust
    // Color filter: primary mode bounding box pre-filter
    let is_distance_sort = sort_by == "distance";
    if let Some(ref cp) = color_params {
        if cp.mode == "primary" {
            let radius = cp.max_dist.sqrt();
            query = query
                .filter(image::Column::PrimaryL.is_not_null())
                .filter(image::Column::PrimaryL.between(cp.lab[0] - radius, cp.lab[0] + radius))
                .filter(image::Column::PrimaryA.between(cp.lab[1] - radius, cp.lab[1] + radius))
                .filter(image::Column::PrimaryB.between(cp.lab[2] - radius, cp.lab[2] + radius));
        }
    }
```

- [ ] **Step 3: Modify the sort/offset/limit block to handle distance sort**

Replace the block (lines 294-312) with:

```rust
    // Popularity sort is handled at application level; distance sort too
    let is_popularity = sort_by == "popularity";
    let is_distance_sort = sort_by == "distance" && color_params.is_some();

    if !is_popularity && !is_distance_sort {
        let sort_column = match sort_by {
            "width" => image::Column::Width,
            "height" => image::Column::Height,
            "aspect_ratio" => image::Column::AspectRatio,
            "source_created_at" => image::Column::SourceCreatedAt,
            "created_at" => image::Column::CreatedAt,
            _ => image::Column::Id,
        };
        if desc {
            query = query.order_by_desc(sort_column);
        } else {
            query = query.order_by_asc(sort_column);
        }
        query = query.offset(offset).limit(limit);
    }
```

- [ ] **Step 4: Add color post-filter + distance sort logic (primary mode)**

After the `query.all(db).await?` line (around line 314), add the color filtering logic. Replace the results block (lines 314-335) with:

```rust
    let results = query.all(db).await?;

    // Color post-filter + distance computation
    let results: Vec<_> = if let Some(ref cp) = color_params {
        if cp.mode == "primary" {
            // Primary mode: filter by exact distance, attach distance score
            results
                .into_iter()
                .filter_map(|(img, auth)| {
                    let l = img.primary_l?;
                    let a = img.primary_a.unwrap_or(0.0);
                    let b = img.primary_b.unwrap_or(0.0);
                    let dist = lab_sq_dist(l, a, b, cp.lab[0], cp.lab[1], cp.lab[2]);
                    if dist <= cp.max_dist {
                        Some(((img, auth), dist))
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            // Palette mode: match via image_color_palette table
            let candidate_ids: Vec<i32> = results.iter().map(|(img, _)| img.id).collect();

            if candidate_ids.is_empty() {
                Vec::new()
            } else {
                use crate::db::entities::image_color_palette::{self, Entity as PaletteEntity};

                let radius = cp.max_dist.sqrt();
                let palette_rows = PaletteEntity::find()
                    .filter(image_color_palette::Column::ImageId.is_in(candidate_ids))
                    .filter(image_color_palette::Column::LabL.between(cp.lab[0] - radius, cp.lab[0] + radius))
                    .filter(image_color_palette::Column::LabA.between(cp.lab[1] - radius, cp.lab[1] + radius))
                    .filter(image_color_palette::Column::LabB.between(cp.lab[2] - radius, cp.lab[2] + radius))
                    .all(db)
                    .await?;

                // Group by image_id, keep min distance
                let mut best_by_image: std::collections::HashMap<i32, f64> =
                    std::collections::HashMap::new();
                for p in &palette_rows {
                    let dist = lab_sq_dist(
                        p.lab_l, p.lab_a, p.lab_b,
                        cp.lab[0], cp.lab[1], cp.lab[2],
                    );
                    if dist <= cp.max_dist {
                        let entry = best_by_image.entry(p.image_id).or_insert(dist);
                        if dist < *entry {
                            *entry = dist;
                        }
                    }
                }

                results
                    .into_iter()
                    .filter_map(|(img, auth)| {
                        best_by_image.get(&img.id).map(|&dist| ((img, auth), dist))
                    })
                    .collect()
            }
        }
    } else {
        // No color filter — no distance score
        results.into_iter().map(|r| (r, 0.0f64)).collect()
    };
```

- [ ] **Step 5: Modify popularity/distance sort + pagination block**

Replace the existing popularity sort block (lines 317-335) with:

```rust
    // For popularity / distance sort: compute scores, sort, paginate
    let results: Vec<_> = if is_popularity || is_distance_sort {
        let mut scored: Vec<(f64, (image::Model, Option<author::Model>))> = results
            .into_iter()
            .map(|((img, auth), color_dist)| {
                let score = if is_popularity {
                    popularity_score(&img)
                } else {
                    // Distance sort: lower is better, so negate for descending
                    -color_dist
                };
                (score, (img, auth))
            })
            .collect();

        // Both popularity and distance: higher score = better
        // For distance, score is negated, so descending = closest first
        if desc {
            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        } else {
            scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        }
        scored
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|(_, pair)| pair)
            .collect()
    } else {
        // Color-filtered but not distance-sorted: drop the distance score
        results
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|((img, auth), _)| (img, auth))
            .collect()
    };
```

- [ ] **Step 6: Verify compilation**

```bash
cargo check -p randimg-core 2>&1
```
Expected: compiles successfully.

- [ ] **Step 7: Commit**

```bash
git add crates/randimg-core/src/db/query/image.rs
git commit -m "feat: add color filtering to list_images query (primary + palette + distance sort)"
```

---

### Task 6: Verify full build and run tests

**Files:**
- None (verification only)

- [ ] **Step 1: Full build**

```bash
cargo build -p randimg-core -p randimg-server 2>&1
```
Expected: compiles successfully with no warnings.

- [ ] **Step 2: Run existing tests**

```bash
cargo test -p randimg-core -- --skip kmeans --skip extract_theme --skip histogram --skip palette --skip primary_color --skip lab_round --skip color_test 2>&1
```
Expected: all tests pass.

- [ ] **Step 3: Commit (if any test fixes needed)**

```bash
# Only if test fixes were needed
git add -A && git commit -m "test: fix tests for color filter changes"
```

---

### Task 7: Final commit and verify

- [ ] **Step 1: Review all changes**

```bash
git diff --stat main
```

- [ ] **Step 2: Check for any warnings**

```bash
cargo clippy -p randimg-core -- -D warnings 2>&1 | tail -20
```

- [ ] **Step 3: Clean up**

```bash
cargo fmt -- -p randimg-core
```
