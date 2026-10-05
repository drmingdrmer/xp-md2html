# Golden Master Testing for xp-md2html

Visual regression testing system for HTML/SVG rendering using Chrome headless.

## Directory Structure

```
tests/
├── fixtures/           # Input test files (HTML, SVG)
├── golden/             # Reference images (auto-generated)
├── debug/              # Debug images (generated during test runs)
└── integration/        # Test code
```

## How It Works

1. **First Run**: Generates golden reference image
2. **Subsequent Runs**: Compares new render against golden image using RMS similarity (1 - root mean square error of grayscale pixels)
3. **Pass/Fail**: Test passes if similarity exceeds threshold

## Running Tests

```bash
# All tests
cargo test golden_master_tests -- --nocapture

# Individual tests
cargo test test_simple_html_rendering -- --nocapture

# Update golden images: delete them, then re-run the tests
rm tests/golden/*.png
cargo test golden_master_tests -- --nocapture
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
| `test_simple_html_rendering` | `simple.html` | 0.80 | 800x600 |
| `test_styled_html_rendering` | `styled.html` | 0.80 | 800x400 |
| `test_svg_rendering` | `svg.svg` | 0.80 | 400x300 |
| `test_different_dimensions` | `simple.html` | 0.80 | 1200x800 |

## Failure Handling

Failed tests save actual image as `{test_name}.actual.png` and show similarity score vs threshold.

## Adding New Tests

1. Create input file in `fixtures/`
2. Add test function with appropriate threshold
3. Run to generate golden image
4. Commit both files

## Dependencies

- Chrome (headless rendering)
- ImageMagick (post-processing)
- Consistent fonts across systems

## Notes

- Tests run in parallel (~1-2s each)
- Golden images are platform-dependent
- Debug images excluded from git
 