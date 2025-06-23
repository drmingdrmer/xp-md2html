use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use image::GenericImageView;
use image_compare::Algorithm;
use xp_md2html::render::with_chrome::WithChrome;

/// Golden master test configuration
struct GoldenTest {
    name: &'static str,
    input_file: &'static str,
    mime_type: &'static str,
    width: u32,
    height: u32,
    similarity_threshold: f64,
}

/// Struct contains fixtures, golden and debug paths
struct TestPaths {
    fixtures_dir: PathBuf,
    golden_dir: PathBuf,
    debug_dir: PathBuf,
}

/// Helper function to get test paths
fn get_test_paths(_test_name: &str) -> TestPaths {
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

/// Compare two images using SSIM (Structural Similarity Index)
fn compare_images(expected_path: &Path, actual_data: &[u8], threshold: f64) -> Result<()> {
    let expected_image = image::open(expected_path)?;
    let actual_image = image::load_from_memory(actual_data)?;

    // Ensure images have the same dimensions
    if expected_image.dimensions() != actual_image.dimensions() {
        anyhow::bail!(
            "Image dimensions differ: expected {:?}, got {:?}",
            expected_image.dimensions(),
            actual_image.dimensions()
        );
    }

    // Convert to grayscale for comparison
    let expected_gray = expected_image.to_luma8();
    let actual_gray = actual_image.to_luma8();

    // Calculate structural similarity
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
    let paths = get_test_paths(test.name);

    // Read input content
    let input_path = paths.fixtures_dir.join(test.input_file);
    let input_content = fs::read_to_string(&input_path)
        .with_context(|| format!("Failed to read input file: {}", input_path.display()))?;

    // Render the image
    let actual_data = WithChrome::render_markup(
        test.mime_type,
        &input_content,
        "png",
        Some(test.width),
        Some(test.height),
        None,
    )
    .await?;

    // Always save debug copy to tests/debug
    {
        let debug_path = paths.debug_dir.join(format!("{}.actual.png", test.name));
        fs::write(&debug_path, &actual_data)?;
        println!("🔍 Debug image saved: {}", debug_path.display());
    }

    // Golden file path
    let golden_path = paths.golden_dir.join(format!("{}.png", test.name));

    if !golden_path.exists() {
        // Create golden file on first run
        fs::write(&golden_path, &actual_data)?;
        println!("✨ Created golden file: {}", golden_path.display());
        println!("   Re-run the test to perform comparison.");
        return Ok(());
    }

    // Compare with golden image
    compare_images(&golden_path, &actual_data, test.similarity_threshold)?;

    println!("✅ Golden test '{}' passed", test.name);
    Ok(())
}

// Individual test functions
#[tokio::test]
async fn test_simple_html_rendering() {
    let test = GoldenTest {
        name: "simple_html",
        input_file: "simple.html",
        mime_type: "text/html",
        width: 800,
        height: 600,
        similarity_threshold: 0.95,
    };

    run_golden_test(&test).await.unwrap();
}

#[tokio::test]
async fn test_styled_html_rendering() {
    let test = GoldenTest {
        name: "styled_html",
        input_file: "styled.html",
        mime_type: "text/html",
        width: 800,
        height: 400,
        similarity_threshold: 0.93, // Slightly lower threshold for complex styling
    };

    run_golden_test(&test).await.unwrap();
}

#[tokio::test]
async fn test_svg_rendering() {
    let test = GoldenTest {
        name: "svg_test",
        input_file: "svg.svg",
        mime_type: "image/svg+xml",
        width: 400,
        height: 300,
        similarity_threshold: 0.95,
    };

    run_golden_test(&test).await.unwrap();
}

#[tokio::test]
async fn test_different_dimensions() {
    let test = GoldenTest {
        name: "simple_large",
        input_file: "simple.html",
        mime_type: "text/html",
        width: 1200,
        height: 800,
        similarity_threshold: 0.95,
    };

    run_golden_test(&test).await.unwrap();
}

// Test that demonstrates failure handling (should fail on purpose)
#[tokio::test]
#[ignore] // Run with: cargo test test_failure_demo -- --ignored
async fn test_failure_demo() {
    let test = GoldenTest {
        name: "simple_html",            // Using existing golden but different input
        input_file: "simple_test.html", // Different input file
        mime_type: "text/html",
        width: 800,
        height: 600,
        similarity_threshold: 0.95,
    };

    // This should fail because we're using a different input file
    // but comparing against the existing simple_html.png golden image
    run_golden_test(&test).await.unwrap();
}
