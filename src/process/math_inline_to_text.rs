//! `math-inline-to-text`: replace every inline formula with Unicode text, such as `x²` for `$x^2$`,
//! as md2zhihu's `math_inline_to_plaintext` does for Weibo, which renders no formula.

use anyhow::Context;
use comrak::nodes::NodeValue;
use comrak::Node;
use techxt::convert::Recovery;
use techxt::convert::UnknownMacroResolution;
use techxt::mathfmt::FontStyle;
use techxt::mathfmt::FontStyleKind;
use techxt::mathfmt::MathWrapDelims;
use techxt::Converter;

/// Replace every `$..$` formula under `root`, also in the `` $`..`$ `` syntax, with its text; a
/// `$$..$$` formula stays.
pub fn apply(root: Node<'_>) -> anyhow::Result<()> {
    let converter = text_converter()?;
    for node in root.descendants() {
        let mut ast = node.data_mut();
        let NodeValue::Math(math) = &ast.value else {
            continue;
        };
        if math.display_math {
            continue;
        }
        let text = to_text(&converter, &math.literal)?;
        ast.value = NodeValue::Text(text.into());
    }
    Ok(())
}

/// The converter of a formula to text. techxt is the Rust successor of `pylatexenc`, which
/// k3down2's `tex_to_plain` uses.
fn text_converter() -> anyhow::Result<Converter> {
    let converter = Converter::builder()
        // `x`, as k3down2 writes it, not the math italic `𝑥`.
        .math_font(FontStyle::Style(FontStyleKind::Upright))
        .math_expression_in(MathWrapDelims::None)
        // A malformed formula is an error instead of half-converted text, while a macro that
        // techxt does not know is left out.
        .recovery(Recovery::Strict)
        .unknown_macro_resolution(UnknownMacroResolution::Accept)
        .build()?;
    Ok(converter)
}

/// The Unicode text of the inline formula `tex`.
fn to_text(converter: &Converter, tex: &str) -> anyhow::Result<String> {
    let source = format!("${tex}$");
    let conversion = converter
        .latex_to_text(&source)
        .with_context(|| format!("Failed to convert formula to text: {source}"))?;
    Ok(conversion.text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use comrak::Arena;

    use super::*;

    /// Inline math, also in code syntax, becomes text; a `$$` formula stays.
    #[test]
    fn test_apply() -> anyhow::Result<()> {
        let markdown = "Let $\\alpha_1 \\le x^2$ in $`\\mathbb{R}^n`$.\n\n$$\ny^2\n$$\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        apply(root)?;

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        assert_eq!(out, "Let α₁ ≤ x² in ℝⁿ.\n\n$$\ny^2\n$$\n");
        Ok(())
    }

    /// A malformed formula is an error that names the formula.
    #[test]
    fn test_apply_malformed() {
        let markdown = "Let $\\frac{a}$.\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        let result = apply(root);

        let error = format!("{:#}", result.unwrap_err());
        let expected = "Failed to convert formula to text: $\\frac{a}$: \
                        missing mandatory argument ‘denominator’";
        assert_eq!(error, expected);
    }

    /// The cases of k3down2's `test_tex_to_plain` give the same text.
    #[test]
    fn test_to_text() -> anyhow::Result<()> {
        let converter = text_converter()?;
        let cases = [
            ("_1", "₁"),
            ("a_1 + b^2", "a₁ + b²"),
            ("b^3 a_z pp", "b³ a_z pp"),
            ("\\mathbb{Q}^3", "ℚ³"),
            ("\\mathbb{Q}^{x+1}", "ℚˣ⁺¹"),
            ("\\sum_{1}^{x+1}(i^2)", "∑₁ˣ⁺¹(i²)"),
        ];
        for (tex, want) in cases {
            let text = to_text(&converter, tex)?;
            assert_eq!(text, want, "{tex}");
        }
        Ok(())
    }
}
