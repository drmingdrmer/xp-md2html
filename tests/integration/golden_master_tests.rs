use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use image::DynamicImage;
use image::RgbImage;
use image::Rgba;
use image::RgbaImage;
use image_compare::Algorithm;
use xp_md2html::render::chrome::ChromeRenderer;
use xp_md2html::render::chrome::OutputFormat;
use xp_md2html::render::chrome::RenderConfig;

/// An unchanged page scores 1.0; a one-character typo in `simple.html` scores about 0.88.
const SIMILARITY_THRESHOLD: f64 = 0.99;

/// Set this environment variable to `1` to save each render as its golden image instead of comparing.
const UPDATE_GOLDEN_ENV: &str = "UPDATE_GOLDEN";

/// The backgrounds that both images are laid over for the comparison: a change of alpha alone
/// shows over at least one of them.
const BACKGROUNDS: [(&str, Rgba<u8>); 2] = [
    ("white", Rgba([255, 255, 255, 255])),
    ("black", Rgba([0, 0, 0, 255])),
];

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

/// Compare two images using RMS similarity (1 - root mean square error of the red, the green or
/// the blue channel, whichever is lowest), with both images laid over each of [`BACKGROUNDS`].
fn compare_images(expected: &RgbaImage, actual: &RgbaImage, threshold: f64) -> Result<()> {
    let expected_dims = expected.dimensions();
    let actual_dims = actual.dimensions();

    if expected_dims != actual_dims {
        anyhow::bail!(
            "Image dimensions differ: expected {:?}, got {:?}",
            expected_dims,
            actual_dims
        );
    }

    for (name, background) in BACKGROUNDS {
        let expected_rgb = lay_over(expected, background);
        let actual_rgb = lay_over(actual, background);

        // Calculate RMS similarity
        let result = image_compare::rgb_similarity_structure(
            &Algorithm::RootMeanSquared,
            &expected_rgb,
            &actual_rgb,
        )?;

        println!("Image similarity score over {name}: {:.4}", result.score);

        if result.score < threshold {
            anyhow::bail!(
                "Image similarity {:.4} over {name} below threshold {:.4}",
                result.score,
                threshold
            );
        }
    }

    Ok(())
}

/// `image` laid over an image of `background` alone, by the alpha of each pixel.
fn lay_over(image: &RgbaImage, background: Rgba<u8>) -> RgbImage {
    let mut laid = RgbaImage::from_pixel(image.width(), image.height(), background);
    image::imageops::overlay(&mut laid, image, 0, 0);
    DynamicImage::ImageRgba8(laid).to_rgb8()
}

/// Run a golden master test
fn run_golden_test(test: &GoldenTest) -> Result<()> {
    let result = do_run_golden_test(test);

    if let Err(e) = &result {
        println!("🔴 Golden test '{}' failed: {}", test.name(), e);
    } else {
        println!("✅ Golden test '{}' passed", test.name());
    }

    result
}

/// Run a golden master test
fn do_run_golden_test(test: &GoldenTest) -> Result<()> {
    let paths = get_test_paths();

    // Read input content
    let input_path = paths.fixtures_dir.join(test.input_file);
    let input_content = fs::read_to_string(&input_path)
        .with_context(|| format!("Failed to read input file: {}", input_path.display()))?;

    // Render the image
    let config = RenderConfig {
        mime: test.mime_type.to_string(),
        output_type: OutputFormat::Png,
        width: test.width,
        height: test.height,
        scale: 1,
        asset_base: None,
    };
    let renderer = ChromeRenderer::new(config)?;
    let actual_data = renderer.render_markup(&input_content)?;

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
    let expected_image = image::open(&golden_path)?.to_rgba8();
    let actual_image = image::load_from_memory(&actual_data)?.to_rgba8();
    compare_images(&expected_image, &actual_image, test.similarity_threshold)?;

    println!("✅ Golden test '{}' passed", test.name());
    Ok(())
}

// Individual test functions
#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "golden images are made on macOS")]
fn test_simple_html_rendering() {
    let test = GoldenTest {
        input_file: "simple.html",
        mime_type: "text/html",
        width: 800,
        height: 600,
        similarity_threshold: SIMILARITY_THRESHOLD,
    };

    run_golden_test(&test).unwrap();
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "golden images are made on macOS")]
fn test_styled_html_rendering() {
    let test = GoldenTest {
        input_file: "styled.html",
        mime_type: "text/html",
        width: 800,
        height: 400,
        similarity_threshold: SIMILARITY_THRESHOLD,
    };

    run_golden_test(&test).unwrap();
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "golden images are made on macOS")]
fn test_svg_rendering() {
    let test = GoldenTest {
        input_file: "svg.svg",
        mime_type: "image/svg+xml",
        width: 400,
        height: 300,
        similarity_threshold: SIMILARITY_THRESHOLD,
    };

    run_golden_test(&test).unwrap();
}

/// A render of `simple_test.html`, a changed copy of `simple.html`, fails the comparison with the
/// golden image of `simple.html`.
#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "golden images are made on macOS")]
fn test_failure_demo() {
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
        output_type: OutputFormat::Png,
        width: test.width,
        height: test.height,
        scale: 1,
        asset_base: None,
    };
    let renderer = ChromeRenderer::new(config).unwrap();
    let actual_data = renderer.render_markup(&input_content).unwrap();

    // This should fail because we're using a different input file
    // but comparing against the existing simple.png golden image
    let golden_path = paths.golden_dir.join("simple.png");
    let expected_image = image::open(&golden_path).unwrap().to_rgba8();
    let actual_image = image::load_from_memory(&actual_data).unwrap().to_rgba8();
    let compared = compare_images(&expected_image, &actual_image, test.similarity_threshold);
    let message = compared.unwrap_err().to_string();
    assert_eq!(
        message,
        "Image dimensions differ: expected (184, 58), got (316, 65)"
    );
}

/// A change of color that keeps the gray level, a change of alpha alone, and a change of size each
/// fail the comparison, and an equal image passes; no Chrome renders these images.
#[test]
fn test_compare_images() {
    // `to_luma8` turns both colors into the same gray, 54.
    let red = RgbaImage::from_pixel(4, 3, Rgba([255, 0, 0, 255]));
    let green = RgbaImage::from_pixel(4, 3, Rgba([0, 76, 0, 255]));
    let white = RgbaImage::from_pixel(4, 3, Rgba([255, 255, 255, 255]));
    let clear_white = RgbaImage::from_pixel(4, 3, Rgba([255, 255, 255, 0]));
    let tall_red = RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255]));

    let same = compare_images(&red, &red, SIMILARITY_THRESHOLD);
    assert!(same.is_ok(), "{same:?}");

    // The expected image, the actual image, and the error of the comparison.
    let cases = [
        (
            &red,
            &green,
            "Image similarity 0.0000 over white below threshold 0.9900",
        ),
        (
            &white,
            &clear_white,
            "Image similarity 0.0000 over black below threshold 0.9900",
        ),
        (
            &red,
            &tall_red,
            "Image dimensions differ: expected (4, 3), got (4, 4)",
        ),
    ];
    for (expected, actual, expected_error) in cases {
        let compared = compare_images(expected, actual, SIMILARITY_THRESHOLD);
        let error = compared.unwrap_err().to_string();
        assert_eq!(error, expected_error);
    }
}
