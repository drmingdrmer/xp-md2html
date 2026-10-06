# XPMD - HTML to Image Converter

Convert HTML/SVG to images and PDFs using headless Chrome.

## Prerequisites

- Chrome/Chromium browser
- ImageMagick: `brew install imagemagick` (macOS) or `sudo apt install imagemagick` (Linux)

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

### Options

```
-i, --input <INPUT>    Input file (HTML/SVG)
-o, --output <OUTPUT>  Output file
-f, --format <FORMAT>  png, jpg, jpeg, pdf [default: output file's extension]
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
