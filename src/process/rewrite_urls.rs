//! `rewrite-image-urls=/REGEX/REPL/` and `rewrite-link-urls=/REGEX/REPL/`: rewrite the URL of every
//! image, or of every link, with a regex, as md2zhihu's `--rewrite` does for the images it stores.

use std::str::FromStr;

use anyhow::Context;
use comrak::nodes::NodeValue;
use comrak::Node;
use fancy_regex::Captures;
use fancy_regex::Expander;
use fancy_regex::Regex;

/// The form of a rule, for the error about a malformed one.
const RULE_FORM: &str = "the rule must be /REGEX/REPL/, with a character that REGEX and REPL \
                         do not hold in place of each /";

/// A `/REGEX/REPL/` rule, whose first character is the delimiter. REGEX has Python's syntax and
/// REPL writes a group as `\1` or `\g<name>`, as in Python's `re.sub`, so md2zhihu's `--rewrite`
/// arguments work unchanged.
#[derive(Clone, Debug)]
pub struct UrlRewrite {
    regex: Regex,
    replacement: String,
}

impl PartialEq for UrlRewrite {
    fn eq(&self, other: &Self) -> bool {
        self.regex.as_str() == other.regex.as_str() && self.replacement == other.replacement
    }
}

impl Eq for UrlRewrite {}

impl FromStr for UrlRewrite {
    type Err = String;

    fn from_str(rule: &str) -> Result<Self, String> {
        let mut chars = rule.chars();
        let delimiter = chars.next().ok_or_else(|| RULE_FORM.to_string())?;
        let parts: Vec<&str> = chars.as_str().split(delimiter).collect();
        let &[pattern, replacement, ""] = parts.as_slice() else {
            return Err(RULE_FORM.to_string());
        };

        let regex =
            Regex::new(pattern).map_err(|error| format!("invalid regex: {pattern}: {error}"))?;
        Expander::python()
            .check(replacement, &regex)
            .map_err(|error| format!("invalid replacement: {replacement}: {error}"))?;
        Ok(Self {
            regex,
            replacement: replacement.to_string(),
        })
    }
}

impl UrlRewrite {
    /// `url` with every match of REGEX replaced by REPL.
    fn rewrite(&self, url: &str) -> anyhow::Result<String> {
        let expander = Expander::python();
        let replace = |captures: &Captures<'_>| expander.expansion(&self.replacement, captures);
        let rewritten = self
            .regex
            .try_replacen(url, 0, replace)
            .with_context(|| format!("Failed to rewrite {url} with {}", self.regex.as_str()))?;
        Ok(rewritten.into_owned())
    }
}

/// Rewrite the URL of every image under `root` with `rule`.
pub fn apply_to_images(root: Node<'_>, rule: &UrlRewrite) -> anyhow::Result<()> {
    for node in root.descendants() {
        let mut ast = node.data_mut();
        let NodeValue::Image(image) = &mut ast.value else {
            continue;
        };
        let url = rule.rewrite(&image.url)?;
        image.url = url;
    }
    Ok(())
}

/// Rewrite the URL of every link under `root` with `rule`. A link whose text is its URL, such as
/// `<https://a.com>`, gets the new URL as its text too, so it still shows where it goes.
pub fn apply_to_links(root: Node<'_>, rule: &UrlRewrite) -> anyhow::Result<()> {
    for node in root.descendants() {
        let mut ast = node.data_mut();
        let NodeValue::Link(link) = &mut ast.value else {
            continue;
        };
        let url = rule.rewrite(&link.url)?;
        replace_url_text(node, &link.url, &url);
        link.url = url;
    }
    Ok(())
}

/// Replace the text of `link` with `new_url` when the text is `old_url` alone.
fn replace_url_text(link: Node<'_>, old_url: &str, new_url: &str) {
    let Some(child) = link.first_child() else {
        return;
    };
    let only_child = child.next_sibling().is_none();
    if !only_child {
        return;
    }
    let mut ast = child.data_mut();
    let NodeValue::Text(text) = &mut ast.value else {
        return;
    };
    if text.as_ref() != old_url {
        return;
    }
    *text = new_url.to_string().into();
}

#[cfg(test)]
mod tests {
    use comrak::Arena;

    use super::*;

    /// A rule takes any delimiter, and Python's `\1` for a group.
    #[test]
    fn test_rewrite() -> anyhow::Result<()> {
        let rule = UrlRewrite::from_str("#^(a)/#\\1x/#").map_err(anyhow::Error::msg)?;

        let url = rule.rewrite("a/p.png")?;
        assert_eq!(url, "ax/p.png");
        Ok(())
    }

    /// A rule without three delimiters, a bad regex, or a group that the regex lacks is an error.
    #[test]
    fn test_from_str_error() {
        let empty = UrlRewrite::from_str("");
        assert_eq!(empty, Err(RULE_FORM.to_string()));

        let open = UrlRewrite::from_str("|a|b");
        assert_eq!(open, Err(RULE_FORM.to_string()));

        let bad_regex = UrlRewrite::from_str("|(|b|");
        let expected_bad_regex = "invalid regex: (: \
                                  Parsing error at position 1: Opening parenthesis without closing parenthesis";
        assert_eq!(bad_regex, Err(expected_bad_regex.to_string()));

        let bad_group = UrlRewrite::from_str("|a|\\1|");
        let expected_bad_group = "invalid replacement: \\1: \
                                  Error compiling regex: Invalid back reference to group 1";
        assert_eq!(bad_group, Err(expected_bad_group.to_string()));
    }

    /// Every image URL that the regex matches changes, a remote one too; a link stays.
    #[test]
    fn test_apply_to_images() -> anyhow::Result<()> {
        let markdown = "![a](assets/x.png) ![b](https://e.com/y.png) [c](assets/z.png)\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);
        let rule = UrlRewrite::from_str("|\\.png$|.webp|").map_err(anyhow::Error::msg)?;

        apply_to_images(root, &rule)?;

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected = "![a](assets/x.webp) ![b](https://e.com/y.webp) [c](assets/z.png)\n";
        assert_eq!(out, expected);
        Ok(())
    }

    /// Every link URL that the regex matches changes, and a link whose text is its URL shows the
    /// new URL; an image stays.
    #[test]
    fn test_apply_to_links() -> anyhow::Result<()> {
        let markdown = "[a](https://old.com/a) <https://old.com/b> [c](c.md) \
                        ![d](https://old.com/d.png)\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);
        let rule = UrlRewrite::from_str("|^https://old\\.com/|https://new.com/|")
            .map_err(anyhow::Error::msg)?;

        apply_to_links(root, &rule)?;

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        let expected = "[a](https://new.com/a) <https://new.com/b> [c](c.md) \
                        ![d](https://old.com/d.png)\n";
        assert_eq!(out, expected);
        Ok(())
    }
}
