//! Code to one HTML page with syntax colors, through syntect.

use std::path::Path;

use anyhow::Context;
use syntect::highlighting::Theme;
use syntect::highlighting::ThemeSet;
use syntect::html::highlighted_html_for_string;
use syntect::parsing::SyntaxSet;

/// The look of the page that [`code_to_html`] builds.
pub struct CodeStyle {
    /// The colors: one of syntect's built-in themes, or a loaded `.tmTheme` file.
    pub theme: Theme,
    /// The width in pixels at which a line wraps.
    pub width: u32,
}

/// The theme that `name` names: one of syntect's built-in themes, or the `.tmTheme` file at that path.
pub fn load_theme(name: &str) -> anyhow::Result<Theme> {
    let mut themes = ThemeSet::load_defaults().themes;
    if let Some(theme) = themes.remove(name) {
        return Ok(theme);
    }

    let path = Path::new(name);
    if path.exists() {
        return ThemeSet::get_theme(path)
            .with_context(|| format!("Failed to load theme file: {name}"));
    }

    let names: Vec<&str> = themes.keys().map(String::as_str).collect();
    anyhow::bail!(
        "Unknown theme: {name}. Built-in themes: {}",
        names.join(", ")
    )
}

/// One HTML page that shows `code` with the colors of `lang`; plain text when `lang` is unknown.
///
/// Every color is an inline `style=`, so the `<pre>` keeps its look when it is pasted into an
/// editor that drops `<style>` blocks. The `<pre>` is as wide as its longest line, up to
/// `style.width`, so a trim cuts a screenshot of the page down to the box.
pub fn code_to_html(lang: Option<&str>, code: &str, style: &CodeStyle) -> anyhow::Result<String> {
    let syntax_set = SyntaxSet::load_defaults_newlines();
    let syntax = lang
        .and_then(|lang| syntax_set.find_syntax_by_token(lang))
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let pre = highlighted_html_for_string(code, &syntax_set, syntax, &style.theme)?;

    // A block without a language is often an ASCII diagram, which reads better with tight lines.
    let line_height = if lang.is_some() { "1.8" } else { "1.3" };
    let width = style.width;

    let page = format!(
        "<!DOCTYPE html>\n\
         <html><head><meta charset=\"utf-8\"><style>\n\
         body {{ margin: 0; padding: 8px; background: transparent; }}\n\
         pre {{ display: inline-block; box-sizing: border-box; max-width: {width}px; margin: 0; padding: 16px; border-radius: 6px; \
         font-family: Menlo, Consolas, \"DejaVu Sans Mono\", monospace; font-size: 14px; line-height: {line_height}; \
         white-space: pre-wrap; word-break: break-all; }}\n\
         </style></head><body>\n\
         {pre}\
         </body></html>\n"
    );
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ocean_dark(width: u32) -> CodeStyle {
        let theme = load_theme("base16-ocean.dark").unwrap();
        CodeStyle { theme, width }
    }

    /// `rust` and its extension `rs` select the same grammar; a keyword gets its own colored span.
    #[test]
    fn test_code_to_html_rust() -> anyhow::Result<()> {
        let style = ocean_dark(1000);
        let html = code_to_html(Some("rust"), "fn main() {}\n", &style)?;

        let expected_pre = "<pre style=\"background-color:#2b303b;\">\n\
            <span style=\"color:#b48ead;\">fn </span>\
            <span style=\"color:#8fa1b3;\">main</span>\
            <span style=\"color:#c0c5ce;\">() {}\n</span>\
            </pre>\n";
        assert!(html.contains(expected_pre), "{html}");
        assert!(html.contains("max-width: 1000px;"));
        assert!(html.contains("line-height: 1.8;"));

        let by_extension = code_to_html(Some("rs"), "fn main() {}\n", &style)?;
        assert_eq!(by_extension, html);
        Ok(())
    }

    /// Without a language the text is escaped, in the theme's text color, with tight lines.
    #[test]
    fn test_code_to_html_plain() -> anyhow::Result<()> {
        let style = ocean_dark(600);
        let html = code_to_html(None, "<a> & b\n", &style)?;

        let expected_pre = "<pre style=\"background-color:#2b303b;\">\n\
            <span style=\"color:#c0c5ce;\">&lt;a&gt; &amp; b\n</span>\
            </pre>\n";
        assert!(html.contains(expected_pre), "{html}");
        assert!(html.contains("max-width: 600px;"));
        assert!(html.contains("line-height: 1.3;"));

        let unknown = code_to_html(Some("no-such-language"), "<a> & b\n", &style)?;
        assert!(unknown.contains(expected_pre), "{unknown}");
        assert!(unknown.contains("line-height: 1.8;"));
        Ok(())
    }

    #[test]
    fn test_load_theme() {
        let theme = load_theme("InspiredGitHub").unwrap();
        assert_eq!(theme.name.as_deref(), Some("GitHub"));

        let error = load_theme("nope").unwrap_err().to_string();
        assert_eq!(
            error,
            "Unknown theme: nope. Built-in themes: InspiredGitHub, Solarized (dark), Solarized (light), \
             base16-eighties.dark, base16-mocha.dark, base16-ocean.dark, base16-ocean.light"
        );
    }
}
