# Golden Master Testing for xp-md2html

Visual regression testing system for HTML/SVG rendering using Chrome headless.

## Directory Structure

```
tests/
├── fixtures/           # Input test files (HTML, SVG)
├── golden/             # Reference images rendered on macOS (auto-generated)
├── debug/              # Debug images (generated during test runs)
└── integration/        # Test code
```

## How It Works

1. **With `UPDATE_GOLDEN=1`**: Saves each render as its golden reference image
2. **Normal Runs**: Compares new render against golden image using RMS similarity (1 - root mean square error of the red, green or blue channel, whichever is lowest), with both images laid over a white and then a black background, so a change of color or of alpha alone fails
3. **Pass/Fail**: Test passes if both images have the same size and the similarity is at least the threshold. A missing golden image fails the test

## Running Tests

```bash
# All tests
cargo test golden_master_tests -- --nocapture

# Individual tests
cargo test test_simple_html_rendering -- --nocapture

# Update golden images
UPDATE_GOLDEN=1 cargo test golden_master_tests -- --nocapture
```

## Test Configuration

```rust
struct GoldenTest {
    input_file: &'static str,        // File in fixtures/; its stem names the golden image
    mime_type: &'static str,         // Rendering type
    width: u32, height: u32,         // Dimensions
    similarity_threshold: f64,       // Required similarity (0.0-1.0)
}
```

## Current Tests

| Test | Input | Threshold | Size |
|------|-------|-----------|------|
| `test_simple_html_rendering` | `simple.html` | 0.99 | 800x600 |
| `test_styled_html_rendering` | `styled.html` | 0.99 | 800x400 |
| `test_svg_rendering` | `svg.svg` | 0.99 | 400x300 |

## Failure Handling

Failed tests save actual image as `debug/{test_name}.actual.png` and show similarity score vs threshold.

## Adding New Tests

1. Create input file in `fixtures/`
2. Add test function with appropriate threshold
3. Run with `UPDATE_GOLDEN=1` to generate golden image
4. Commit both files

## Dependencies

- Chrome (headless rendering)
- ImageMagick (post-processing)
- macOS: golden tests are ignored on other OSes

## Notes

- Tests run in parallel (~1-2s each)
- Golden images are platform-dependent
- Debug images excluded from git
 