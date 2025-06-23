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
xpmd render -i input.html -o output.png [OPTIONS]
```

### Options

```
-i, --input <INPUT>    Input file (HTML/SVG)
-o, --output <OUTPUT>  Output file
-f, --format <FORMAT>  png, jpg, jpeg, pdf [default: png]
-w, --width <WIDTH>    Window width [default: 1000]
    --height <HEIGHT>  Window height [default: 2000]
-m, --mime <MIME>      MIME type (auto-detected)
-b, --base <BASE>      Base path for assets
```

## Examples

```bash
# Basic conversion
xpmd render -i page.html -o screenshot.png

# Custom size and format
xpmd render -i page.html -o document.pdf -f pdf -w 1200 --height 800

# SVG with assets
xpmd render -i diagram.svg -o diagram.png -b /path/to/assets
```

# TODO

- golend_test 里不需要name
- 测试不同尺寸的图片是否能对比出正确的相似度，如果不可以的话，可能要先在先统一尺寸
- with Chrome 这个 structure 先设定一个宽度大小，再新建的时候就把 Chrome 路径什么都准备好，而不是在执行的时候准备，执行的时候只接受一个参数，就是输入的内容。
