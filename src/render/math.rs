//! LaTeX math to SVG or an image, through MathJax inside Chrome.

use anyhow::Context;

use crate::render::chrome::ChromeRenderer;
use crate::render::page::element_text;
use crate::render::page::PADDING;

/// MathJax 3.2.2, TeX input and SVG output with every extension, so nothing loads from the network.
const MATHJAX: &str = include_str!("../../vendor/mathjax/tex-svg-full.js");

/// One page that typesets `tex` once with MathJax's SVG output; `display` picks the display style
/// over the inline style.
///
/// The TeX sits in a hidden element that MathJax reads as text, so it needs only HTML escaping.
/// `fontCache: 'local'` keeps every glyph path inside the SVG, so the SVG needs no font.
/// After the load event MathJax adds assistive MathML beside the SVG, and Chrome would show it as
/// a second formula, because the page does not carry MathJax's stylesheet; the page hides it.
/// The script writes the right and the bottom edge of the SVG, in CSS pixels, into the hidden
/// `#size`, so that an image can get a window that fits the formula.
pub fn math_page(tex: &str, display: bool) -> String {
    let escaped = tex
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<!DOCTYPE html>\n\
         <html><head><meta charset=\"utf-8\"><style>\n\
         body {{ margin: 0; padding: {PADDING}px; }}\n\
         mjx-assistive-mml {{ display: none; }}\n\
         #size {{ display: none; }}\n\
         </style></head><body>\n\
         <div id=\"tex\" hidden>{escaped}</div>\n\
         <div id=\"out\"></div>\n\
         <div id=\"size\"></div>\n\
         <script>\n\
         window.MathJax = {{\n\
           svg: {{ fontCache: 'local' }},\n\
           options: {{ enableMenu: false }},\n\
           startup: {{\n\
             typeset: false,\n\
             ready() {{\n\
               MathJax.startup.defaultReady();\n\
               const tex = document.getElementById('tex').textContent;\n\
               const node = MathJax.tex2svg(tex, {{ display: {display} }});\n\
               document.getElementById('out').appendChild(node);\n\
               const box = node.querySelector('svg').getBoundingClientRect();\n\
               const size = Math.ceil(box.right) + ' ' + Math.ceil(box.bottom);\n\
               document.getElementById('size').textContent = size;\n\
             }}\n\
           }}\n\
         }};\n\
         </script>\n\
         <script>{MATHJAX}</script>\n\
         </body></html>\n"
    )
}

/// The SVG that MathJax makes of `tex`; every glyph is a path inside it, so it stands alone.
pub fn math_to_svg(renderer: &ChromeRenderer, tex: &str, display: bool) -> anyhow::Result<String> {
    let dom = renderer.dump_dom(&math_page(tex, display))?;
    extract_svg(&dom)
}

/// The image of `tex` that `renderer` makes, in a window that fits the formula plus [`PADDING`]; a
/// fixed window would cut a wide or tall formula off.
///
/// One run of Chrome measures the formula on the page, and a second one takes the screenshot.
pub fn math_to_image(
    renderer: &ChromeRenderer,
    tex: &str,
    display: bool,
) -> anyhow::Result<Vec<u8>> {
    let page = math_page(tex, display);
    let dom = renderer.dump_dom(&page)?;
    let (right, bottom) = formula_edges(&dom)?;
    let fitted = renderer.with_window(
        right.saturating_add(PADDING),
        bottom.saturating_add(PADDING),
    );
    fitted.render_markup(&page)
}

/// The right and the bottom edge of the formula, in CSS pixels, that the script of [`math_page`]
/// wrote into `#size` of `dom`.
fn formula_edges(dom: &str) -> anyhow::Result<(u32, u32)> {
    let size = element_text(dom, "size").context("The page has no #size element")?;
    let Some((right, bottom)) = size.split_once(' ') else {
        anyhow::bail!("The page measured no formula: {size:?}");
    };
    let right = right
        .parse()
        .with_context(|| format!("The formula's right edge is not a number: {right}"))?;
    let bottom = bottom
        .parse()
        .with_context(|| format!("The formula's bottom edge is not a number: {bottom}"))?;
    Ok((right, bottom))
}

/// The `<svg>` element that MathJax put inside its `<mjx-container>` in `dom`.
fn extract_svg(dom: &str) -> anyhow::Result<String> {
    let container = dom
        .find("<mjx-container")
        .context("MathJax left no <mjx-container> in the page")?;
    let after = &dom[container..];
    let start = after
        .find("<svg")
        .context("MathJax left no <svg> in the page")?;
    let end = after
        .find("</svg>")
        .context("MathJax left no </svg> in the page")?;
    Ok(after[start..end + "</svg>".len()].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_math_page() {
        let page = math_page("a < b & c", true);
        assert!(
            page.contains("<div id=\"tex\" hidden>a &lt; b &amp; c</div>"),
            "{page}"
        );
        assert!(page.contains("{ display: true }"));

        let inline = math_page("x", false);
        assert!(inline.contains("{ display: false }"));
    }

    #[test]
    fn test_extract_svg() -> anyhow::Result<()> {
        let dom = "<html><body><div id=\"out\">\
            <mjx-container class=\"MathJax\" jax=\"SVG\" display=\"true\">\
            <svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1ex\"><defs></defs><g><path d=\"M0\"></path></g></svg>\
            </mjx-container></div></body></html>";
        let svg = extract_svg(dom)?;
        assert_eq!(
            svg,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1ex\"><defs></defs><g><path d=\"M0\"></path></g></svg>"
        );

        let error = extract_svg("<html><body></body></html>").unwrap_err();
        assert_eq!(
            error.to_string(),
            "MathJax left no <mjx-container> in the page"
        );
        Ok(())
    }

    #[test]
    fn test_formula_edges() -> anyhow::Result<()> {
        let dom = "<html><body><div id=\"out\"></div><div id=\"size\">2958 26</div></body></html>";
        let edges = formula_edges(dom)?;
        assert_eq!(edges, (2958, 26));

        let error = formula_edges("<html><body><div id=\"size\"></div></body></html>").unwrap_err();
        assert_eq!(error.to_string(), "The page measured no formula: \"\"");
        Ok(())
    }
}
