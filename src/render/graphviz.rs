//! A DOT graph to SVG or an image, through viz.js inside Chrome.

use anyhow::Context;

use crate::render::chrome::ChromeRenderer;
use crate::render::page::element_text;
use crate::render::page::escape_text;
use crate::render::page::unescape_text;

/// viz.js 3.31.0: Graphviz 16.1.0 compiled to WebAssembly, in one file that loads nothing from the network.
const VIZ: &str = include_str!("../../vendor/viz/viz-global.js");

/// One page that lays out `source` once with the `dot` engine and puts the SVG into `#out`, or
/// Graphviz's error message into `#err`.
///
/// The source sits in a hidden element that the script reads as text, so it needs only HTML escaping.
///
/// viz.js compiles its WebAssembly in the background, and Chrome dumps the DOM once the page has
/// loaded, so on a slow machine the dump came before the SVG. The page makes the compile
/// synchronous, which keeps the whole run inside its scripts, before the load event.
pub fn graphviz_page(source: &str) -> String {
    let escaped = escape_text(source);
    format!(
        "<!DOCTYPE html>\n\
         <html><head><meta charset=\"utf-8\"></head><body>\n\
         <div id=\"dot\" hidden>{escaped}</div>\n\
         <div id=\"out\"></div>\n\
         <div id=\"err\"></div>\n\
         <script>\n\
         WebAssembly.instantiate = (bytes, imports) => {{\n\
           const module = new WebAssembly.Module(bytes);\n\
           const instance = new WebAssembly.Instance(module, imports);\n\
           return Promise.resolve({{ module, instance }});\n\
         }};\n\
         </script>\n\
         <script>{VIZ}</script>\n\
         <script>\n\
         Viz.instance().then(viz => {{\n\
           const source = document.getElementById('dot').textContent;\n\
           const svg = viz.renderSVGElement(source, {{ engine: 'dot' }});\n\
           document.getElementById('out').appendChild(svg);\n\
         }}).catch(error => {{\n\
           document.getElementById('err').textContent = error.message;\n\
         }});\n\
         </script>\n\
         </body></html>\n"
    )
}

/// The SVG that Graphviz makes of `source`; a DOT error becomes the error.
pub fn graphviz_to_svg(renderer: &ChromeRenderer, source: &str) -> anyhow::Result<String> {
    let dom = renderer.dump_dom(&graphviz_page(source))?;
    extract_svg(&dom)
}

/// The `<svg>` element inside `#out` in `dom`, or the error that `#err` holds.
fn extract_svg(dom: &str) -> anyhow::Result<String> {
    let error = element_text(dom, "err").context("viz.js left no #err element in the page")?;
    if !error.is_empty() {
        anyhow::bail!("Graphviz: {}", unescape_text(error));
    }
    let out = element_text(dom, "out").context("viz.js left no #out element in the page")?;
    let start = out
        .find("<svg")
        .context("viz.js left no <svg> in the page")?;
    let end = out
        .find("</svg>")
        .context("viz.js left no </svg> in the page")?;
    Ok(out[start..end + "</svg>".len()].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graphviz_page() {
        let page = graphviz_page("digraph { a -> b [label=\"x & y\"]; }");
        assert!(
            page.contains(
                "<div id=\"dot\" hidden>digraph { a -&gt; b [label=\"x &amp; y\"]; }</div>"
            ),
            "{page}"
        );
        assert!(page.contains("viz.renderSVGElement(source, { engine: 'dot' })"));
    }

    #[test]
    fn test_extract_svg() -> anyhow::Result<()> {
        let dom = "<html><body><div id=\"dot\" hidden>digraph {}</div>\
            <div id=\"out\"><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"8pt\" height=\"8pt\">\n\
            <g><polygon fill=\"white\"></polygon></g></svg></div>\
            <div id=\"err\"></div></body></html>";
        let svg = extract_svg(dom)?;
        assert_eq!(
            svg,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"8pt\" height=\"8pt\">\n\
             <g><polygon fill=\"white\"></polygon></g></svg>"
        );

        let dom = "<html><body><div id=\"out\"></div>\
            <div id=\"err\">syntax error in line 1 near '-&gt;'</div></body></html>";
        let error = extract_svg(dom).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Graphviz: syntax error in line 1 near '->'"
        );

        let error = extract_svg("<html><body></body></html>").unwrap_err();
        assert_eq!(error.to_string(), "viz.js left no #err element in the page");
        Ok(())
    }
}
