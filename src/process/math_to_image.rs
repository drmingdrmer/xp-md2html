//! `math-to-image`: render every formula to a PNG and link the PNG where the formula was;
//! `math-to-image=SERVICE` links the image of an online formula service instead.

use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;

use super::code_to_image::MATH_LANG;
use super::mermaid_to_image::replace_code_blocks;
use super::ActionContext;
use crate::render::math::math_to_image;
use crate::render::math_img::math_url;
use crate::render::math_img::MathService;

/// Replace every `$..$` and `$$..$$` formula and every ```` ```math ```` block under `root` with a
/// PNG that `ctx.renderer` renders into `ctx.assets_dir`, or, with a `service`, with the image at
/// the URL where the service draws the formula.
pub fn apply<'a>(
    arena: &'a Arena<'a>,
    root: Node<'a>,
    service: Option<MathService>,
    ctx: &ActionContext,
) -> anyhow::Result<()> {
    let image_url = |tex: &str, display: bool| -> anyhow::Result<String> {
        if let Some(service) = service {
            return Ok(math_url(service, tex, display));
        }
        render(tex, display, ctx)
    };

    replace_code_blocks(arena, root, MATH_LANG, |tex| image_url(tex, true))?;
    replace_formulas(arena, root, image_url)
}

/// Replace every `$..$` and `$$..$$` formula under `root` with an image whose URL `image_url`
/// returns for the formula's TeX and style; a display formula alone in its paragraph leaves a
/// paragraph with one image.
pub fn replace_formulas<'a>(
    arena: &'a Arena<'a>,
    root: Node<'a>,
    mut image_url: impl FnMut(&str, bool) -> anyhow::Result<String>,
) -> anyhow::Result<()> {
    // Edits while walking would confuse the walk, so collect the formulas first.
    let formulas: Vec<(Node<'a>, String, bool)> = root
        .descendants()
        .filter_map(|node| {
            let (tex, display) = formula_of(node)?;
            Some((node, tex, display))
        })
        .collect();

    for (formula, tex, display) in formulas {
        let url = image_url(&tex, display)?;
        let image = super::image_node(arena, url);
        formula.insert_after(image);
        formula.detach();
    }
    Ok(())
}

/// The TeX of `node` and whether it is in the display style, when `node` is a formula.
fn formula_of(node: Node<'_>) -> Option<(String, bool)> {
    let ast = node.data();
    let NodeValue::Math(math) = &ast.value else {
        return None;
    };
    Some((math.literal.clone(), math.display_math))
}

/// Render `tex`, in the display style when `display` is set, to a PNG in `ctx.assets_dir` and
/// return the link to the PNG.
fn render(tex: &str, display: bool, ctx: &ActionContext) -> anyhow::Result<String> {
    let renderer = ctx.renderer.get()?;

    // The display style draws some formulas bigger, so the name hashes the delimiters too.
    let markdown = if display {
        format!("$${tex}$$")
    } else {
        format!("${tex}$")
    };
    ctx.render_once("math", &markdown, || math_to_image(renderer, tex, display))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inline math, also in code syntax, becomes an inline image, and a display formula alone in
    /// its paragraph becomes a paragraph with one image, also inside a list.
    #[test]
    fn test_replace_formulas() -> anyhow::Result<()> {
        let markdown = "Inline $x$ and $`y`$.\n\n$$\nz\n$$\n\n- $$w$$\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        let mut seen = Vec::new();
        replace_formulas(&arena, root, |tex, display| {
            seen.push((tex.to_string(), display));
            Ok(format!("m{}.png", seen.len()))
        })?;
        let expected_seen = vec![
            ("x".to_string(), false),
            ("y".to_string(), false),
            ("\nz\n".to_string(), true),
            ("w".to_string(), true),
        ];
        assert_eq!(seen, expected_seen);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        assert_eq!(
            out,
            "Inline ![](m1.png) and ![](m2.png).\n\n![](m3.png)\n\n- ![](m4.png)\n"
        );
        Ok(())
    }
}
