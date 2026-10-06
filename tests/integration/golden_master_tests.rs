use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use image::GenericImageView;
use image_compare::Algorithm;
use xp_md2html::render::chrome::ChromeRenderer;
use xp_md2html::render::chrome::RenderConfig;

/// An unchanged page scores 1.0; a one-character typo in `simple.html` scores about 0.88.
const SIMILARITY_THRESHOLD: f64 = 0.99;

/// Set this environment variable to `1` to save each render as its golden image instead of comparing.
const UPDATE_GOLDEN_ENV: &str = "UPDATE_GOLDEN";

/// Golden master test configuration
struct GoldenTest {
    input_file: &'static str,
    mime_type: &'static str,
    width: u32,
    height: u32,
    similarity_threshold: f64,
}

impl GoldenTest {
    fn name(&self) -> &str {
        // remove the suffix from the input file
        self.input_file
            .rsplit_once('.')
            .map(|(name, _)| name)
            .unwrap_or(self.input_file)
    }
}

/// Struct contains fixtures, golden and debug paths
struct TestPaths {
    fixtures_dir: PathBuf,
    golden_dir: PathBuf,
    debug_dir: PathBuf,
}

/// Helper function to get test paths
fn get_test_paths() -> TestPaths {
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    let fixtures_dir = root_dir.join("tests/fixtures");
    let golden_dir = root_dir.join("tests/golden");
    let debug_dir = root_dir.join("tests/debug");

    // Ensure directories exist
    fs::create_dir_all(&fixtures_dir).unwrap();
    fs::create_dir_all(&golden_dir).unwrap();
    fs::create_dir_all(&debug_dir).unwrap();

    TestPaths {
        fixtures_dir,
        golden_dir,
        debug_dir,
    }
}

/// Compare two images using RMS similarity (1 - root mean square error of grayscale pixels)
fn compare_images(expected_path: &Path, actual_data: &[u8], threshold: f64) -> Result<()> {
    let expected_image = image::open(expected_path)?;
    let actual_image = image::load_from_memory(actual_data)?;

    let expected_dims = expected_image.dimensions();
    let actual_dims = actual_image.dimensions();

    if expected_dims != actual_dims {
        anyhow::bail!(
            "Image dimensions differ: expected {:?}, got {:?}",
            expected_dims,
            actual_dims
        );
    }

    // Convert to grayscale for comparison
    let expected_gray = expected_image.to_luma8();
    let actual_gray = actual_image.to_luma8();

    // Calculate RMS similarity
    let result = image_compare::gray_similarity_structure(
        &Algorithm::RootMeanSquared,
        &expected_gray,
        &actual_gray,
    )?;

    println!("Image similarity score: {:.4}", result.score);

    if result.score < threshold {
        // Save the actual image for debugging
        let debug_path = expected_path.with_extension("actual.png");
        actual_image.save(&debug_path)?;

        anyhow::bail!(
            "Image similarity {:.4} below threshold {:.4}. Actual image saved to: {}",
            result.score,
            threshold,
            debug_path.display()
        );
    }

    Ok(())
}

/// Run a golden master test
async fn run_golden_test(test: &GoldenTest) -> Result<()> {
    let result = do_run_golden_test(test).await;

    if let Err(e) = &result {
        println!("🔴 Golden test '{}' failed: {}", test.name(), e);
    } else {
        println!("✅ Golden test '{}' passed", test.name());
    }

    result
}

/// Run a golden master test
async fn do_run_golden_test(test: &GoldenTest) -> Result<()> {
    let paths = get_test_paths();

    // Read input content
    let input_path = paths.fixtures_dir.join(test.input_file);
    let input_content = fs::read_to_string(&input_path)
        .with_context(|| format!("Failed to read input file: {}", input_path.display()))?;

    // Render the image
    let config = RenderConfig {
        mime: test.mime_type.to_string(),
        output_type: "png".to_string(),
        width: test.width,
        height: test.height,
        asset_base: None,
    };
    let renderer = ChromeRenderer::new(config)?;
    let actual_data = renderer.render_markup(&input_content).await?;

    // Always save debug copy to tests/debug
    {
        let debug_path = paths.debug_dir.join(format!("{}.actual.png", test.name()));
        fs::write(&debug_path, &actual_data)?;
        println!("🔍 Debug image saved: {}", debug_path.display());
    }

    // Golden file path
    let golden_path = paths.golden_dir.join(format!("{}.png", test.name()));

    let update_golden = std::env::var(UPDATE_GOLDEN_ENV).as_deref() == Ok("1");
    if update_golden {
        fs::write(&golden_path, &actual_data)?;
        println!("✨ Updated golden file: {}", golden_path.display());
        return Ok(());
    }

    if !golden_path.exists() {
        anyhow::bail!(
            "Golden file is missing: {}. Run with {}=1 to create it.",
            golden_path.display(),
            UPDATE_GOLDEN_ENV
        );
    }

    // Compare with golden image
    compare_images(&golden_path, &actual_data, test.similarity_threshold)?;

    println!("✅ Golden test '{}' passed", test.name());
    Ok(())
}

// Individual test functions
#[tokio::test]
#[cfg_attr(not(target_os = "macos"), ignore = "golden images are made on macOS")]
async fn test_simple_html_rendering() {
    let test = GoldenTest {
        input_file: "simple.html",
        mime_type: "text/html",
        width: 800,
        height: 600,
        similarity_threshold: SIMILARITY_THRESHOLD,
    };

    run_golden_test(&test).await.unwrap();
}

#[tokio::test]
#[cfg_attr(not(target_os = "macos"), ignore = "golden images are made on macOS")]
async fn test_styled_html_rendering() {
    let test = GoldenTest {
        input_file: "styled.html",
        mime_type: "text/html",
        width: 800,
        height: 400,
        similarity_threshold: SIMILARITY_THRESHOLD,
    };

    run_golden_test(&test).await.unwrap();
}

#[tokio::test]
#[cfg_attr(not(target_os = "macos"), ignore = "golden images are made on macOS")]
async fn test_svg_rendering() {
    let test = GoldenTest {
        input_file: "svg.svg",
        mime_type: "image/svg+xml",
        width: 400,
        height: 300,
        similarity_threshold: SIMILARITY_THRESHOLD,
    };

    run_golden_test(&test).await.unwrap();
}

#[tokio::test]
#[cfg_attr(not(target_os = "macos"), ignore = "golden images are made on macOS")]
async fn test_different_dimensions() {
    let test = GoldenTest {
        input_file: "simple.html",
        mime_type: "text/html",
        width: 1200,
        height: 800,
        similarity_threshold: SIMILARITY_THRESHOLD,
    };

    run_golden_test(&test).await.unwrap();
}

// Test that demonstrates failure handling (should fail on purpose)
#[tokio::test]
#[ignore] // Run with: cargo test test_failure_demo -- --ignored
async fn test_failure_demo() {
    let test = GoldenTest {
        input_file: "simple_test.html", // Different input file
        mime_type: "text/html",
        width: 800,
        height: 600,
        similarity_threshold: SIMILARITY_THRESHOLD,
    };

    let paths = get_test_paths();

    let input_path = paths.fixtures_dir.join(test.input_file);
    let input_content = fs::read_to_string(&input_path).unwrap();

    let config = RenderConfig {
        mime: test.mime_type.to_string(),
        output_type: "png".to_string(),
        width: test.width,
        height: test.height,
        asset_base: None,
    };
    let renderer = ChromeRenderer::new(config).unwrap();
    let actual_data = renderer.render_markup(&input_content).await.unwrap();

    // This should fail because we're using a different input file
    // but comparing against the existing simple.png golden image
    let golden_path = paths.golden_dir.join("simple.png");
    compare_images(&golden_path, &actual_data, test.similarity_threshold).unwrap();
}
