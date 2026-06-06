# Remove Manga Example Design

**Date:** 2026-06-06

## Goal

Create an independent example binary that removes all manga images (where `illust_type = "manga"`) from the system, including OSS storage, local storage, and database records. Supports dry run mode for safety.

## Requirements

- Query all images where `illust_type = "manga"` and `deleted_at IS NULL`
- Delete from DogeCloud OSS (using `image_path` as S3 key)
- Delete local files from `IMAGE_DIR`
- Hard delete from database (not soft delete)
- Remove associated records: `image_tag_association`, `image_color_palette`
- Default to dry run mode; require `--execute` flag for actual deletion
- Output detailed logging of each operation

## Design

### File Structure

```
examples/remove_manga.rs  # Single file example
```

### CLI Interface

```bash
# Dry run (default) - shows what would be deleted
cargo run --example remove_manga

# Execute actual deletion
cargo run --example remove_manga -- --execute
```

### Implementation Flow

1. **Initialize**: Load config via `AppConfig::from_env()`, create `WorkerState`
2. **Query**: Find all images with `illust_type = "manga"` AND `deleted_at IS NULL`
3. **Process each image**:
   - Log image details (id, title, image_path)
   - If not dry run:
     - Delete from OSS: `oss.delete(&img.image_path)`
     - Delete local file: `tokio::fs::remove_file(path)`
     - Delete associated records: `image_tag_association`, `image_color_palette`
     - Hard delete image: `DELETE FROM images WHERE id = ?`
4. **Summary**: Print total count, success count, failure count

### Database Queries

```sql
-- Find manga images
SELECT id, title, image_path FROM images 
WHERE illust_type = 'manga' AND deleted_at IS NULL;

-- Delete associations (before image)
DELETE FROM image_tag_association WHERE image_id = ?;
DELETE FROM image_color_palette WHERE image_id = ?;

-- Hard delete image
DELETE FROM images WHERE id = ?;
```

### Error Handling

- OSS deletion failure: Log error, continue to next image
- Local file not found: Log warning, continue (file may already be deleted)
- Database error: Log error, continue
- Track failure count for summary

### Output Format

```
[DRY RUN] Found 42 manga images to remove
  - Image #123: "manga_title" (path: abc/123.webp)
  - Image #456: "another_manga" (path: def/456.webp)
  ...
[DRY RUN] Would delete 42 images. Run with --execute to proceed.

--- OR with --execute ---

[EXECUTE] Removing 42 manga images...
  ✓ Deleted OSS: abc/123.webp
  ✓ Deleted local: ./images/abc/123.webp
  ✓ Deleted DB: Image #123
  ✗ Failed OSS: def/456.webp (error: ...)
  ...
[EXECUTE] Complete: 41 succeeded, 1 failed
```

## Dependencies

- `randimg-core`: WorkerState, AppConfig, db::query, dogecloud::oss
- `tokio`: Async runtime
- `sea-orm`: Database operations

## Testing

Manual testing only - this is a destructive utility example.
