use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use xp_md2html::process::Action;
use xp_md2html::process::ActionContext;
use xp_md2html::render::chrome::ChromeRenderer;
use xp_md2html::render::chrome::RenderConfig;

#[derive(Parser)]
#[command(name = "xpmd")]
#[command(about = "A markdown to HTML/image converter with Chrome rendering")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Render HTML content to image using headless Chrome
    RenderMarkup(RenderArgs),
    /// Apply actions to a markdown file in order and write the result
    Process(ProcessArgs),
}

/// The options of the `render-markup` subcommand.
#[derive(Args)]
struct RenderArgs {
    /// Input file path (HTML content)
    #[arg(short, long)]
    input: PathBuf,

    /// Output file path
    #[arg(short, long)]
    output: PathBuf,

    /// Output format: png, jpg, jpeg, pdf [default: the output file's extension]
    #[arg(short, long)]
    format: Option<String>,

    /// Window width for rendering
    #[arg(short, long, default_value = "1000")]
    width: u32,

    /// Window height for rendering
    #[arg(long, default_value = "2000")]
    height: u32,

    /// MIME type of input content (auto-detected if not specified)
    #[arg(short, long)]
    mime: Option<String>,

    /// Directory for relative asset paths in HTML input [default: the input file's directory]
    #[arg(short, long)]
    base: Option<PathBuf>,
}

/// The options of the `process` subcommand.
#[derive(Args)]
struct ProcessArgs {
    /// Input markdown file
    #[arg(short, long)]
    input: PathBuf,

    /// Output markdown file
    #[arg(short, long)]
    output: PathBuf,

    /// Directory for the files the actions create [default: the output file's directory]
    #[arg(long)]
    assets: Option<PathBuf>,

    /// Window width for rendering images
    #[arg(short, long, default_value = "1000")]
    width: u32,

    /// Window height for rendering images
    #[arg(long, default_value = "2000")]
    height: u32,

    /// An action to apply, in the given order; one of: table-to-image
    #[arg(long = "action", required = true)]
    actions: Vec<Action>,
}

/// The context of the error when Chrome or ImageMagick is missing.
const INSTALL_HELP: &str = "Failed to render content. Make sure Chrome/Chromium and ImageMagick are installed and accessible.\n\
    Chrome: On macOS: Install from https://www.google.com/chrome/\n\
    Chrome: On Linux: sudo apt install chromium-browser (Ubuntu/Debian) or equivalent\n\
    Chrome: On Windows: Install from https://www.google.com/chrome/\n\
    ImageMagick: On macOS: brew install imagemagick\n\
    ImageMagick: On Linux: sudo apt install imagemagick\n\
    ImageMagick: On Windows: Install from https://imagemagick.org/";

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::RenderMarkup(args) => {
            render_command(args)?;
        }
        Commands::Process(args) => {
            process_command(args)?;
        }
    }

    Ok(())
}

fn render_command(args: RenderArgs) -> Result<()> {
    let RenderArgs {
        input,
        output,
        format,
        width,
        height,
        mime,
        base,
    } = args;

    // Validate input file exists
    if !input.exists() {
        anyhow::bail!("Input file does not exist: {}", input.display());
    }

    // Read input content as string
    let content = fs::read_to_string(&input)
        .with_context(|| format!("Failed to read input file: {}", input.display()))?;

    // Determine MIME type
    let mime_type = mime.unwrap_or_else(|| {
        // Try to determine from file extension
        input
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| match ext.to_lowercase().as_str() {
                "html" | "htm" => "text/html",
                "svg" => "image/svg+xml",
                "xml" => "application/xml",
                _ => "text/html", // Default fallback
            })
            .unwrap_or("text/html")
            .to_string()
    });

    // Relative asset paths in HTML resolve against `--base`, by default the input file's directory.
    let is_html = mime_type.contains("html");
    if base.is_some() && !is_html {
        anyhow::bail!("--base only works with HTML input, not {}", mime_type);
    }
    let absolute_input = std::path::absolute(&input)
        .with_context(|| format!("Failed to resolve input path: {}", input.display()))?;
    let input_dir = absolute_input.parent().map(Path::to_path_buf);
    let base = if is_html { base.or(input_dir) } else { None };

    let format = resolve_format(format.as_deref(), &output)?;

    println!(
        "Rendering {} to {} ({}x{}, format: {})",
        input.display(),
        output.display(),
        width,
        height,
        format
    );

    // Create output directory if it doesn't exist
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory: {}", parent.display()))?;
    }

    // Render using Chrome
    let config = RenderConfig {
        mime: mime_type,
        output_type: format.to_string(),
        width,
        height,
        asset_base: base,
    };
    let renderer = ChromeRenderer::new(config).context(INSTALL_HELP)?;
    let image_data = renderer.render_markup(&content)?;

    // Write output
    fs::write(&output, &image_data)
        .with_context(|| format!("Failed to write output file: {}", output.display()))?;

    println!("✅ Successfully rendered to: {}", output.display());
    println!("📊 Output size: {} bytes", image_data.len());

    Ok(())
}

fn process_command(args: ProcessArgs) -> Result<()> {
    let ProcessArgs {
        input,
        output,
        assets,
        width,
        height,
        actions,
    } = args;

    let markdown = fs::read_to_string(&input)
        .with_context(|| format!("Failed to read input file: {}", input.display()))?;

    let absolute_output = std::path::absolute(&output)
        .with_context(|| format!("Failed to resolve output path: {}", output.display()))?;
    let output_dir = absolute_output
        .parent()
        .with_context(|| format!("Output path has no directory: {}", output.display()))?
        .to_path_buf();
    let stem = absolute_output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .with_context(|| format!("Output path has no file name: {}", output.display()))?
        .to_string();
    let assets_dir = assets.unwrap_or_else(|| output_dir.clone());

    fs::create_dir_all(&output_dir).with_context(|| {
        format!(
            "Failed to create output directory: {}",
            output_dir.display()
        )
    })?;
    fs::create_dir_all(&assets_dir).with_context(|| {
        format!(
            "Failed to create assets directory: {}",
            assets_dir.display()
        )
    })?;

    let config = RenderConfig {
        mime: "text/html".to_string(),
        output_type: "png".to_string(),
        width,
        height,
        asset_base: None,
    };
    let renderer = ChromeRenderer::new(config).context(INSTALL_HELP)?;
    let ctx = ActionContext {
        assets_dir,
        output_dir,
        stem,
        renderer,
    };

    let processed = xp_md2html::process::process_markdown(&markdown, &actions, &ctx)?;
    fs::write(&output, processed)
        .with_context(|| format!("Failed to write output file: {}", output.display()))?;

    println!("✅ Successfully wrote: {}", output.display());

    Ok(())
}

/// The output formats that `-f` and the output file's extension accept, as error messages list them.
const SUPPORTED_FORMATS: &str = "png, jpg, jpeg, pdf";

/// Return the output format that `name`, a `-f` value or a file extension, names.
fn format_of(name: &str) -> Option<&'static str> {
    match name.to_lowercase().as_str() {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpg"),
        "pdf" => Some("pdf"),
        _ => None,
    }
}

/// Return the output format that `-f` names, by default the one that the extension of `output` names.
///
/// When `-f` and the extension both name a format, the two must agree.
fn resolve_format(format: Option<&str>, output: &Path) -> Result<&'static str> {
    let extension = output.extension().and_then(|ext| ext.to_str());
    let extension_format = extension.and_then(format_of);

    let Some(format) = format else {
        return extension_format.with_context(|| {
            format!(
                "Cannot tell the output format from {}. Pass -f with one of: {}",
                output.display(),
                SUPPORTED_FORMATS
            )
        });
    };

    let Some(flag_format) = format_of(format) else {
        anyhow::bail!(
            "Unsupported output format: {}. Supported: {}",
            format,
            SUPPORTED_FORMATS
        );
    };

    let extension_agrees = extension_format.is_none() || extension_format == Some(flag_format);
    if !extension_agrees {
        anyhow::bail!(
            "-f {} does not match the extension of {}",
            format,
            output.display()
        );
    }
    Ok(flag_format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_format_from_extension() -> Result<()> {
        let format = resolve_format(None, Path::new("out.JPG"))?;
        assert_eq!(format, "jpg");

        let format = resolve_format(None, Path::new("out.jpeg"))?;
        assert_eq!(format, "jpg");

        let format = resolve_format(None, Path::new("out.pdf"))?;
        assert_eq!(format, "pdf");
        Ok(())
    }

    #[test]
    fn test_resolve_format_from_flag() -> Result<()> {
        let format = resolve_format(Some("PNG"), Path::new("out"))?;
        assert_eq!(format, "png");

        let format = resolve_format(Some("png"), Path::new("out.tmp"))?;
        assert_eq!(format, "png");

        let format = resolve_format(Some("jpeg"), Path::new("out.jpg"))?;
        assert_eq!(format, "jpg");
        Ok(())
    }

    #[test]
    fn test_resolve_format_errors() {
        let result = resolve_format(None, Path::new("out"));
        let message = result.unwrap_err().to_string();
        assert_eq!(
            message,
            "Cannot tell the output format from out. Pass -f with one of: png, jpg, jpeg, pdf"
        );

        let result = resolve_format(None, Path::new("out.gif"));
        let message = result.unwrap_err().to_string();
        assert_eq!(
            message,
            "Cannot tell the output format from out.gif. Pass -f with one of: png, jpg, jpeg, pdf"
        );

        let result = resolve_format(Some("png"), Path::new("out.jpg"));
        let message = result.unwrap_err().to_string();
        assert_eq!(message, "-f png does not match the extension of out.jpg");

        let result = resolve_format(Some("gif"), Path::new("out.gif"));
        let message = result.unwrap_err().to_string();
        assert_eq!(
            message,
            "Unsupported output format: gif. Supported: png, jpg, jpeg, pdf"
        );
    }
}
