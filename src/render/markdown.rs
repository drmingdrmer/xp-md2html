//! Markdown to HTML: the bare HTML of the content, or a page that shows it in GitHub's style.

use comrak::Options;

/// GitHub's markdown style sheet; it styles the element with the class `markdown-body`.
const GITHUB_CSS: &str = include_str!("../../github-markdown.css");

/// The options that turn markdown into HTML: the ones `xpmd process` parses with, and raw HTML
/// kept as github.com keeps it.
pub fn html_options() -> Options<'static> {
    let mut options = crate::process::gfm_math_options();
    options.render.r#unsafe = true;
    options
}

/// The bare HTML of `markdown`, with no page around it.
pub fn markdown_to_html(markdown: &str) -> String {
    comrak::markdown_to_html(markdown, &html_options())
}

/// A page that shows `html` in GitHub's style, at the width of a github.com page.
pub fn markdown_page(html: &str) -> String {
    format!(
        "<!DOCTYPE html>\n\
         <html><head><meta charset=\"utf-8\"><style>\n\
         {GITHUB_CSS}\n\
         body {{ margin: 0; }}\n\
         .markdown-body {{ box-sizing: border-box; max-width: 980px; margin: 0 auto; padding: 45px; }}\n\
         </style></head><body>\n\
         <article class=\"markdown-body\">\n\
         {html}\
         </article>\n\
         </body></html>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_to_html() {
        let markdown = "| a | b |\n|---|---|\n| 1<br>2 | $x$ |\n";
        let html = markdown_to_html(markdown);
        assert_eq!(
            html,
            "<table>\n\
             <thead>\n\
             <tr>\n\
             <th>a</th>\n\
             <th>b</th>\n\
             </tr>\n\
             </thead>\n\
             <tbody>\n\
             <tr>\n\
             <td>1<br>2</td>\n\
             <td><span data-math-style=\"inline\">x</span></td>\n\
             </tr>\n\
             </tbody>\n\
             </table>\n"
        );
    }

    #[test]
    fn test_markdown_page() {
        let page = markdown_page("<p>hi</p>\n");
        assert!(page.starts_with("<!DOCTYPE html>\n<html><head>"), "{page}");
        assert!(page.contains(".markdown-body {\n"), "{page}");
        assert!(
            page.ends_with(
                "<article class=\"markdown-body\">\n<p>hi</p>\n</article>\n</body></html>\n"
            ),
            "{page}"
        );
    }
}
