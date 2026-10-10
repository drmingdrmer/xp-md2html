# XPMD - HTML to Image Converter

Convert HTML/SVG to images and PDFs using headless Chrome.

## Prerequisites

- Chrome/Chromium browser
- ImageMagick, for PNG and JPEG output: `brew install imagemagick` (macOS) or `sudo apt install imagemagick` (Linux)

Set `XPMD_CHROME` or `XPMD_MAGICK` to a path, or to a command in `PATH`, to choose the Chrome or the ImageMagick that xpmd runs.

## Installation

```bash
git clone <repository-url>
cd xp-md2html
cargo build --release --bin xpmd
```

## Usage

```bash
xpmd render-markup -i input.html -o output.png [OPTIONS]
```

Every `render-*` subcommand reads stdin without `-i` and writes stdout without `-o`.

### Options

```
-i, --input <INPUT>    Input file (HTML/SVG) [default: stdin]
-o, --output <OUTPUT>  Output file [default: stdout]
-f, --format <FORMAT>  png, jpg, jpeg, pdf [default: output file's extension, else png]
-w, --width <WIDTH>    Window width [default: 1000]
    --height <HEIGHT>  Window height [default: 2000]
-m, --mime <MIME>      MIME type (auto-detected)
-b, --base <BASE>      Directory for relative asset paths in HTML [default: input file's directory]
```

## Examples

```bash
# Basic conversion
xpmd render-markup -i page.html -o screenshot.png

# PDF with selectable text, printed on Letter pages; -w and --height do not apply
xpmd render-markup -i page.html -o document.pdf -f pdf

# HTML whose relative asset paths point into another directory
xpmd render-markup -i page.html -o page.png -b /path/to/assets
```
