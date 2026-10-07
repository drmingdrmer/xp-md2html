//! A mermaid diagram to SVG or an image, through mermaid inside Chrome.

use anyhow::Context;

use crate::render::chrome::ChromeRenderer;
use crate::render::page::element_text;
use crate::render::page::escape_text;
use crate::render::page::unescape_text;

/// mermaid 12.1.0 with every diagram type in one file, so nothing loads from the network.
const MERMAID: &str = include_str!("../../vendor/mermaid/mermaid.min.js");

/// One page that draws `source` once and puts the SVG into `#out` as text, or mermaid's error
/// message into `#err`.
///
/// The source sits in a hidden element that the script reads as text, so it needs only HTML
/// escaping. The SVG goes into `#out` as text too, so Chrome serializes it unchanged and the
/// `</div>` of the HTML labels inside it cannot end the element early.
pub fn mermaid_page(source: &str) -> String {
    let escaped = escape_text(source);
    format!(
        "<!DOCTYPE html>\n\
         <html><head><meta charset=\"utf-8\"></head><body>\n\
         <div id=\"src\" hidden>{escaped}</div>\n\
         <div id=\"out\"></div>\n\
         <div id=\"err\"></div>\n\
         <script>{MERMAID}</script>\n\
         <script>\n\
         mermaid.initialize({{ startOnLoad: false }});\n\
         const source = document.getElementById('src').textContent;\n\
         mermaid.render('d', source).then(result => {{\n\
           document.getElementById('out').textContent = result.svg;\n\
         }}).catch(error => {{\n\
           document.getElementById('err').textContent = error.message;\n\
         }});\n\
         </script>\n\
         </body></html>\n"
    )
}

/// The SVG that mermaid makes of `source`; a diagram error becomes the error.
pub fn mermaid_to_svg(renderer: &ChromeRenderer, source: &str) -> anyhow::Result<String> {
    let dom = renderer.dump_dom(&mermaid_page(source))?;
    extract_svg(&dom)
}

/// The SVG text inside `#out` in `dom`, or the error that `#err` holds.
fn extract_svg(dom: &str) -> anyhow::Result<String> {
    let error = element_text(dom, "err").context("mermaid left no #err element in the page")?;
    if !error.is_empty() {
        anyhow::bail!("mermaid: {}", unescape_text(error));
    }
    let out = element_text(dom, "out").context("mermaid left no #out element in the page")?;
    let svg = unescape_text(out);
    if !svg.starts_with("<svg") {
        anyhow::bail!("mermaid left no <svg> in the page");
    }
    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mermaid_page() {
        let page = mermaid_page("graph LR\n  A --> B[x & y]\n");
        assert!(
            page.contains("<div id=\"src\" hidden>graph LR\n  A --&gt; B[x &amp; y]\n</div>"),
            "{page}"
        );
        assert!(page.contains("mermaid.render('d', source)"));
    }

    #[test]
    fn test_extract_svg() -> anyhow::Result<()> {
        let dom = "<html><body><div id=\"src\" hidden>graph LR</div>\
            <div id=\"out\">&lt;svg id=\"d\" width=\"100%\"&gt;&lt;foreignObject&gt;&lt;div&gt;a &amp;amp; b&lt;/div&gt;&lt;/foreignObject&gt;&lt;/svg&gt;</div>\
            <div id=\"err\"></div></body></html>";
        let svg = extract_svg(dom)?;
        assert_eq!(
            svg,
            "<svg id=\"d\" width=\"100%\"><foreignObject><div>a &amp; b</div></foreignObject></svg>"
        );

        let dom = "<html><body><div id=\"out\"></div>\
            <div id=\"err\">Parse error on line 2:\n...&gt;</div></body></html>";
        let error = extract_svg(dom).unwrap_err();
        assert_eq!(error.to_string(), "mermaid: Parse error on line 2:\n...>");

        let error = extract_svg("<html><body></body></html>").unwrap_err();
        assert_eq!(
            error.to_string(),
            "mermaid left no #err element in the page"
        );
        Ok(())
    }
}
