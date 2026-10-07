//! A DOT graph to SVG or an image, through viz.js inside Chrome.

use anyhow::Context;

use crate::render::chrome::ChromeRenderer;

/// viz.js 3.31.0: Graphviz 16.1.0 compiled to WebAssembly, in one file that loads nothing from the network.
const VIZ: &str = include_str!("../../vendor/viz/viz-global.js");

/// The pixels around the SVG in the page of [`svg_page`]; the trim removes them again.
pub const PADDING: u32 = 8;

/// The CSS pixels in one point: the SVG that Graphviz makes gives its size in points.
const PIXELS_PER_POINT: f64 = 96.0 / 72.0;

/// One page that lays out `source` once with the `dot` engine and puts the SVG into `#out`, or
/// Graphviz's error message into `#err`.
///
/// The source sits in a hidden element that the script reads as text, so it needs only HTML escaping.
pub fn graphviz_page(source: &str) -> String {
    let escaped = escape_text(source);
    format!(
        "<!DOCTYPE html>\n\
         <html><head><meta charset=\"utf-8\"></head><body>\n\
         <div id=\"dot\" hidden>{escaped}</div>\n\
         <div id=\"out\"></div>\n\
         <div id=\"err\"></div>\n\
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

/// The content of the `<div id="{id}">` element in `dom`, up to its `</div>`.
fn element_text<'a>(dom: &'a str, id: &str) -> Option<&'a str> {
    let open = format!("<div id=\"{id}\">");
    let start = dom.find(&open)? + open.len();
    let after = &dom[start..];
    let end = after.find("</div>")?;
    Some(&after[..end])
}

/// The width and height in CSS pixels of `svg`, from the `width` and `height` attributes in points
/// that Graphviz writes on the root element.
pub fn svg_size(svg: &str) -> anyhow::Result<(u32, u32)> {
    let tag_end = svg.find('>').context("The SVG has no root element")?;
    let root = &svg[..tag_end];
    let width = attribute_points(root, "width")?;
    let height = attribute_points(root, "height")?;
    Ok((width, height))
}

/// The value of the `name="<number>pt"` attribute in `tag`, rounded up to CSS pixels.
fn attribute_points(tag: &str, name: &str) -> anyhow::Result<u32> {
    let prefix = format!(" {name}=\"");
    let start = tag
        .find(&prefix)
        .with_context(|| format!("The SVG root element has no {name} attribute"))?
        + prefix.len();
    let after = &tag[start..];
    let end = after.find('"').context("The SVG root element is cut off")?;
    let value = &after[..end];
    let points = value
        .strip_suffix("pt")
        .with_context(|| format!("The SVG {name} is not in points: {value}"))?;
    let points: f64 = points
        .parse()
        .with_context(|| format!("The SVG {name} is not a number: {value}"))?;
    let pixels = points * PIXELS_PER_POINT;
    Ok(pixels.ceil() as u32)
}

/// A page that shows `svg` with [`PADDING`] pixels around it, for `ChromeRenderer::render_markup`.
pub fn svg_page(svg: &str) -> String {
    format!(
        "<!DOCTYPE html>\n\
         <html><head><meta charset=\"utf-8\"><style>\n\
         body {{ margin: 0; padding: {PADDING}px; }}\n\
         svg {{ display: block; }}\n\
         </style></head><body>\n\
         {svg}\n\
         </body></html>\n"
    )
}

/// `text` with the three characters escaped that HTML text content cannot hold.
fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The text that Chrome serialized as `text`; it escapes `&`, `<`, `>` and the no-break space.
fn unescape_text(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", "\u{a0}")
        .replace("&amp;", "&")
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

    #[test]
    fn test_svg_size() -> anyhow::Result<()> {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"307pt\" height=\"44pt\" viewBox=\"0.00 0.00 307.00 44.00\">\n</svg>";
        let size = svg_size(svg)?;
        // 307pt is 409.33px, 44pt is 58.67px.
        assert_eq!(size, (410, 59));

        let error = svg_size("<svg width=\"100%\" height=\"44pt\">").unwrap_err();
        assert_eq!(error.to_string(), "The SVG width is not in points: 100%");

        let error = svg_size("<svg height=\"44pt\">").unwrap_err();
        assert_eq!(
            error.to_string(),
            "The SVG root element has no width attribute"
        );
        Ok(())
    }

    #[test]
    fn test_svg_page() {
        let page = svg_page("<svg></svg>");
        assert!(page.contains("body { margin: 0; padding: 8px; }"), "{page}");
        assert!(page.contains("\n<svg></svg>\n"), "{page}");
    }
}
