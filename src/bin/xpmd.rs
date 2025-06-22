use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use xp_md2html::render::with_chrome::WithChrome;

#[derive(Parser)]
#[command(name = "xpmd")]
#[command(about = "A markdown to HTML/image converter with Chrome rendering")]
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Render HTML content to image using headless Chrome
    Render {
        /// Input file path (HTML content)
        #[arg(short, long)]
        input: PathBuf,

        /// Output file path
        #[arg(short, long)]
        output: PathBuf,

        /// Output format: png, jpg, jpeg, pdf
        #[arg(short, long, default_value = "png")]
        format: String,

        /// Window width for rendering
        #[arg(short, long, default_value = "1000")]
        width: u32,

        /// Window height for rendering  
        #[arg(long, default_value = "2000")]
        height: u32,

        /// MIME type of input content (auto-detected if not specified)
        #[arg(short, long)]
        mime: Option<String>,

        /// Base path for assets (for HTML files with relative paths)
        #[arg(short, long)]
        base: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Render {
            input,
            output,
            format,
            width,
            height,
            mime,
            base,
        } => {
            render_command(input, output, format, width, height, mime, base).await?;
        }
    }

    Ok(())
}

async fn render_command(
    input: PathBuf,
    output: PathBuf,
    format: String,
    width: u32,
    height: u32,
    mime: Option<String>,
    base: Option<PathBuf>,
) -> Result<()> {
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

    // Validate output format
    match format.to_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "pdf" => {}
        _ => anyhow::bail!(
            "Unsupported output format: {}. Supported: png, jpg, jpeg, pdf",
            format
        ),
    }

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
    let image_data = WithChrome::render_markup(
        &mime_type,
        &content,
        &format.to_lowercase(),
        Some(width),
        Some(height),
        base.as_deref(),
    )
    .await
    .with_context(|| {
        "Failed to render content. Make sure Chrome/Chromium and ImageMagick are installed and accessible.\n\
         Chrome: On macOS: Install from https://www.google.com/chrome/\n\
         Chrome: On Linux: sudo apt install chromium-browser (Ubuntu/Debian) or equivalent\n\
         Chrome: On Windows: Install from https://www.google.com/chrome/\n\
         ImageMagick: On macOS: brew install imagemagick\n\
         ImageMagick: On Linux: sudo apt install imagemagick\n\
         ImageMagick: On Windows: Install from https://imagemagick.org/"
    })?;

    // Write output
    fs::write(&output, &image_data)
        .with_context(|| format!("Failed to write output file: {}", output.display()))?;

    println!("✅ Successfully rendered to: {}", output.display());
    println!("📊 Output size: {} bytes", image_data.len());

    Ok(())
}
