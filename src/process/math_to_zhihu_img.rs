//! `math-to-zhihu-img`: replace every formula with an `<img>` of zhihu's equation service, which
//! zhihu's editor takes as an equation.

use comrak::nodes::NodeHtmlBlock;
use comrak::nodes::NodeValue;
use comrak::Node;

use super::code_to_image::MATH_LANG;

/// The CommonMark kind of an HTML block that starts with a tag that is not a block-level tag, such
/// as `<img>`.
const HTML_BLOCK_TYPE_OTHER_TAG: u8 = 7;

/// The bytes other than ASCII letters and digits that Python's `urllib.parse.quote` keeps.
const URL_SAFE_BYTES: &[u8] = b"_.-~/";

/// Replace every `$..$` and `$$..$$` formula under `root` with an inline `<img>` tag and every
/// ```` ```math ```` block with an HTML block that holds one, as md2zhihu's `math_inline_to_imgtag`
/// and `math_block_to_imgtag` do.
pub fn apply(root: Node<'_>) {
    for node in root.descendants() {
        let mut ast = node.data_mut();
        let Some(html) = zhihu_html(&ast.value) else {
            continue;
        };
        ast.value = html;
    }
}

/// The raw HTML that replaces `value` when it is a formula or a ```` ```math ```` block.
fn zhihu_html(value: &NodeValue) -> Option<NodeValue> {
    match value {
        NodeValue::Math(math) => {
            let tag = zhihu_tag(&math.literal, math.display_math);
            Some(NodeValue::HtmlInline(tag))
        }
        NodeValue::CodeBlock(code) => {
            let lang = code.info.split_whitespace().next();
            if lang != Some(MATH_LANG) {
                return None;
            }
            let tag = zhihu_tag(&code.literal, true);
            let block = NodeHtmlBlock {
                block_type: HTML_BLOCK_TYPE_OTHER_TAG,
                literal: format!("{tag}\n"),
            };
            Some(NodeValue::HtmlBlock(block))
        }
        _ => None,
    }
}

/// The `<img>` tag of k3down2's `tex_to_zhihu`: zhihu's equation service draws `tex`, and a
/// trailing `\\` makes zhihu center a `display` formula.
fn zhihu_tag(tex: &str, display: bool) -> String {
    let tex = zhihu_compatible(tex);
    let alt = if display { format!("{tex}\\\\") } else { tex };
    let url = quote(&alt);
    format!(
        "<img src=\"https://www.zhihu.com/equation?tex={url}\" alt=\"{alt}\" \
         class=\"ee_img tr_noresize\" eeimg=\"1\">"
    )
}

/// `tex` on one line, with every `>` written as `\gt`, as k3down2's `tex_to_zhihu_compatible` writes
/// it: a `>` in the alt text breaks a later `\}` on zhihu.
///
/// k3down2 drops each newline and puts `\gt` right before the next character, so `\cdot`, a
/// newline, `x` turns into `\cdotx`, and `x>y` into `x\gty`; zhihu draws both as an undefined
/// command. Here a newline becomes a space, as it does in TeX, and a space separates `\gt` from a
/// letter.
fn zhihu_compatible(tex: &str) -> String {
    let tex = tex.trim();
    let mut chars = tex.chars().peekable();
    let mut prev = None;
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '\n' => out.push(' '),
            '>' if prev != Some('\\') => {
                out.push_str("\\gt");
                let next_is_letter = chars.peek().is_some_and(char::is_ascii_alphabetic);
                if next_is_letter {
                    out.push(' ');
                }
            }
            _ => out.push(c),
        }
        prev = Some(c);
    }
    out
}

/// `text` with every byte other than an ASCII letter, a digit or one of `URL_SAFE_BYTES` written as
/// `%XX`, as Python's `urllib.parse.quote` writes it.
fn quote(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        let safe = byte.is_ascii_alphanumeric() || URL_SAFE_BYTES.contains(&byte);
        if safe {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use comrak::Arena;

    use super::*;

    /// Inline math, also in code syntax, becomes an inline tag; a display formula and a
    /// ```` ```math ```` block become a centered tag; a code block in another language stays.
    #[test]
    fn test_apply() -> anyhow::Result<()> {
        let markdown = "Inline $x$ and $`y`$.\n\n$$\nz\n$$\n\n\
                        ```math\nw\n```\n\n```text\nv\n```\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        apply(root);

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected = format!(
            "Inline {} and {}.\n\n{}\n\n{}\n\n```text\nv\n```\n",
            zhihu_tag("x", false),
            zhihu_tag("y", false),
            zhihu_tag("z", true),
            zhihu_tag("w", true),
        );
        assert_eq!(out, expected);
        Ok(())
    }

    /// The tag is k3down2's, except that a newline inside the TeX becomes a space and a space
    /// separates `\gt` from a letter.
    #[test]
    fn test_zhihu_tag() {
        let k3down2_tag = zhihu_tag("x>1, \\>, a_b~/.-", false);
        let expected_k3down2_tag = r#"<img src="https://www.zhihu.com/equation?tex=x%5Cgt1%2C%20%5C%3E%2C%20a_b~/.-" alt="x\gt1, \>, a_b~/.-" class="ee_img tr_noresize" eeimg="1">"#;
        assert_eq!(k3down2_tag, expected_k3down2_tag);

        let display_tag = zhihu_tag("\n\\text{中}\n= y\n", true);
        let expected_display_tag = r#"<img src="https://www.zhihu.com/equation?tex=%5Ctext%7B%E4%B8%AD%7D%20%3D%20y%5C%5C" alt="\text{中} = y\\" class="ee_img tr_noresize" eeimg="1">"#;
        assert_eq!(display_tag, expected_display_tag);

        let letter_tag = zhihu_tag("x>y", false);
        let expected_letter_tag = r#"<img src="https://www.zhihu.com/equation?tex=x%5Cgt%20y" alt="x\gt y" class="ee_img tr_noresize" eeimg="1">"#;
        assert_eq!(letter_tag, expected_letter_tag);
    }
}
