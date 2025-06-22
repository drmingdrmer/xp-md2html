# XPMD - HTML to Image Converter

Convert HTML content to images and PDFs using headless Chrome.

## Installation

### Prerequisites

Install Chrome/Chromium and ImageMagick:

- **Chrome**: [https://www.google.com/chrome/](https://www.google.com/chrome/)
- **ImageMagick**: 
  - macOS: `brew install imagemagick`
  - Linux: `sudo apt install imagemagick`
  - Windows: [https://imagemagick.org/](https://imagemagick.org/)

### Build

```bash
git clone <repository-url>
cd xp-md2html
cargo build --release --bin xpmd
```

## Usage

```bash
# Basic usage
xpmd render --input page.html --output screenshot.png

# Custom size
xpmd render --input page.html --output screenshot.png --width 1200 --height 800

# Different formats
xpmd render --input page.html --output document.pdf --format pdf
xpmd render --input page.html --output image.jpg --format jpeg
```

### Options

```
-i, --input <INPUT>    Input HTML file
-o, --output <OUTPUT>  Output file
-f, --format <FORMAT>  Output format: png, jpg, jpeg, pdf [default: png]
-w, --width <WIDTH>    Window width [default: 1000]
    --height <HEIGHT>  Window height [default: 2000]
-m, --mime <MIME>      MIME type (auto-detected)
-b, --base <BASE>      Base path for assets
```

## Examples

```bash
# Simple HTML
echo '<h1>Hello World!</h1>' > hello.html
xpmd render -i hello.html -o hello.png

# With custom dimensions
xpmd render -i page.html -o large.png --width 1920 --height 1080

# SVG to PNG
xpmd render -i diagram.svg -o diagram.png --mime "image/svg+xml"

# HTML with local assets
xpmd render -i index.html -o webpage.png --base /path/to/assets
```
