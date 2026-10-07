//! What the pages that draw a diagram inside Chrome share: the text they carry in, the DOM they
//! carry out, and the page that shows the SVG for a screenshot.

use anyhow::Context;

/// The pixels around the SVG in the page of [`svg_page`]; the trim removes them again.
pub const PADDING: u32 = 8;

/// The CSS pixels in one point: the SVG that Graphviz makes gives its size in points.
const PIXELS_PER_POINT: f64 = 96.0 / 72.0;

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

/// The width and height in CSS pixels of `svg`: from the `width` and `height` attributes in
/// points that Graphviz writes on the root element, else from the `viewBox` that mermaid writes.
pub fn svg_size(svg: &str) -> anyhow::Result<(u32, u32)> {
    let tag_end = svg.find('>').context("The SVG has no root element")?;
    let root = &svg[..tag_end];
    let width = attribute(root, "width");
    let height = attribute(root, "height");
    if let (Some(width), Some(height)) = (width, height) {
        let in_points = width.ends_with("pt") && height.ends_with("pt");
        if in_points {
            let width = points_to_pixels(width)?;
            let height = points_to_pixels(height)?;
            return Ok((width, height));
        }
    }
    let view_box = attribute(root, "viewBox")
        .context("The SVG root element has neither width and height in points nor a viewBox")?;
    let fields: Vec<&str> = view_box.split_whitespace().collect();
    let [_, _, width, height] = fields.as_slice() else {
        anyhow::bail!("The SVG viewBox does not hold four numbers: {view_box}");
    };
    let width = units_to_pixels(width)?;
    let height = units_to_pixels(height)?;
    Ok((width, height))
}

/// The value of the `name="..."` attribute in `tag`.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!(" {name}=\"");
    let start = tag.find(&prefix)? + prefix.len();
    let after = &tag[start..];
    let end = after.find('"')?;
    Some(&after[..end])
}

/// `value`, a length with the `pt` suffix, rounded up to CSS pixels.
fn points_to_pixels(value: &str) -> anyhow::Result<u32> {
    let points = value
        .strip_suffix("pt")
        .with_context(|| format!("The SVG length is not in points: {value}"))?;
    let points: f64 = points
        .parse()
        .with_context(|| format!("The SVG length is not a number: {value}"))?;
    let pixels = points * PIXELS_PER_POINT;
    Ok(pixels.ceil() as u32)
}

/// `value`, a number of viewBox units, rounded up to CSS pixels; one unit is one pixel.
fn units_to_pixels(value: &str) -> anyhow::Result<u32> {
    let units: f64 = value
        .parse()
        .with_context(|| format!("The SVG viewBox holds a value that is not a number: {value}"))?;
    Ok(units.ceil() as u32)
}

/// `text` with the three characters escaped that HTML text content cannot hold.
pub fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The text that Chrome serialized as `text`; it escapes `&`, `<`, `>` and the no-break space.
pub fn unescape_text(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", "\u{a0}")
        .replace("&amp;", "&")
}

/// The content of the `<div id="{id}">` element in `dom`, up to its `</div>`.
pub fn element_text<'a>(dom: &'a str, id: &str) -> Option<&'a str> {
    let open = format!("<div id=\"{id}\">");
    let start = dom.find(&open)? + open.len();
    let after = &dom[start..];
    let end = after.find("</div>")?;
    Some(&after[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_svg_page() {
        let page = svg_page("<svg></svg>");
        assert!(page.contains("body { margin: 0; padding: 8px; }"), "{page}");
        assert!(page.contains("\n<svg></svg>\n"), "{page}");
    }

    #[test]
    fn test_svg_size_in_points() -> anyhow::Result<()> {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"307pt\" height=\"44pt\" viewBox=\"0.00 0.00 307.00 44.00\">\n</svg>";
        let size = svg_size(svg)?;
        // 307pt is 409.33px, 44pt is 58.67px.
        assert_eq!(size, (410, 59));
        Ok(())
    }

    #[test]
    fn test_svg_size_from_view_box() -> anyhow::Result<()> {
        let svg = "<svg id=\"d\" width=\"100%\" style=\"max-width: 688.375px;\" viewBox=\"4 4 688.375 187\"></svg>";
        let size = svg_size(svg)?;
        assert_eq!(size, (689, 187));

        let error = svg_size("<svg width=\"100%\" height=\"44pt\"></svg>").unwrap_err();
        assert_eq!(
            error.to_string(),
            "The SVG root element has neither width and height in points nor a viewBox"
        );

        let error = svg_size("<svg viewBox=\"0 0 10\"></svg>").unwrap_err();
        assert_eq!(
            error.to_string(),
            "The SVG viewBox does not hold four numbers: 0 0 10"
        );
        Ok(())
    }

    #[test]
    fn test_escape_and_unescape_text() {
        let escaped = escape_text("a < b & c > d");
        assert_eq!(escaped, "a &lt; b &amp; c &gt; d");

        let unescaped = unescape_text("a &lt; b &amp; c &gt; d&nbsp;&amp;lt;");
        assert_eq!(unescaped, "a < b & c > d\u{a0}&lt;");
    }

    #[test]
    fn test_element_text() {
        let dom = "<body><div id=\"err\"></div><div id=\"out\">x &lt; y</div></body>";
        let out = element_text(dom, "out");
        assert_eq!(out, Some("x &lt; y"));

        let err = element_text(dom, "err");
        assert_eq!(err, Some(""));

        let missing = element_text(dom, "src");
        assert_eq!(missing, None);
    }
}
