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
2. **Subsequent Runs**: Compares new render against golden image using SSIM
3. **Pass/Fail**: Test passes if similarity exceeds threshold

## Running Tests

```bash
# All tests
cargo test golden_master_tests -- --nocapture

# Individual tests
cargo test test_simple_html_rendering -- --nocapture

# Update golden images
cargo test update_golden_images -- --ignored --nocapture
```

## Test Configuration

```rust
struct GoldenTest {
    name: &'static str,              // Test identifier
    input_file: &'static str,        // File in fixtures/
    mime_type: &'static str,         // Rendering type
    width: u32, height: u32,         // Dimensions
    similarity_threshold: f64,       // Required similarity (0.0-1.0)
}
```

## Current Tests

| Test | Input | Threshold | Size |
|------|-------|-----------|------|
| `simple_html` | `simple.html` | 0.95 | 800x600 |
| `styled_html` | `styled.html` | 0.93 | 800x400 |
| `svg_test` | `svg.svg` | 0.95 | 400x300 |
| `simple_large` | `simple.html` | 0.95 | 1200x800 |

## Similarity Thresholds

- **0.95+**: Simple content
- **0.93+**: Complex styling
- **0.90+**: Dynamic content

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
 