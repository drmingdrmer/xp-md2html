//! `table-to-html`: replace every table with a bare `<table>`, which zhihu's editor takes as pasted HTML.

use comrak::nodes::NodeHtmlBlock;
use comrak::nodes::NodeValue;
use comrak::Arena;
use comrak::Node;

use super::table_to_image::is_table;
use crate::render::markdown::html_options;

/// The CommonMark kind of an HTML block that starts with a tag such as `<table>`.
const HTML_BLOCK_TYPE_TAG: u8 = 6;

/// The lines md2zhihu dropped from the HTML; zhihu's editor took the table of bare rows that was left.
const DROPPED_LINES: [&str; 4] = ["<thead>", "</thead>", "<tbody>", "</tbody>"];

/// Replace every table under `root` with an HTML block that holds its `<table>`.
pub fn apply<'a>(arena: &'a Arena<'a>, root: Node<'a>) -> anyhow::Result<()> {
    // Edits while walking would confuse the walk, so collect the tables first.
    let tables: Vec<Node<'a>> = root.descendants().filter(|node| is_table(node)).collect();

    for table in tables {
        let literal = table_html(table)?;
        let block = NodeHtmlBlock {
            block_type: HTML_BLOCK_TYPE_TAG,
            literal,
        };
        let html = arena.alloc(NodeValue::HtmlBlock(block).into());
        table.insert_after(html);
        table.detach();
    }
    Ok(())
}

/// The HTML of `table` as `render-markdown --bare` writes it, without the `<thead>` and `<tbody>` lines.
fn table_html(table: Node<'_>) -> anyhow::Result<String> {
    let mut html = String::new();
    comrak::format_html(table, &html_options(), &mut html)?;
    let kept: Vec<&str> = html
        .lines()
        .filter(|line| !DROPPED_LINES.contains(line))
        .collect();
    let mut joined = kept.join("\n");
    joined.push('\n');
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply() -> anyhow::Result<()> {
        let markdown = "Before.\n\n\
            | a | b |\n\
            |---|--:|\n\
            | `1`<br>x | ![i](i.png) |\n\n\
            After.\n";
        let arena = Arena::new();
        let options = super::super::gfm_math_options();
        let root = comrak::parse_document(&arena, markdown, &options);

        apply(&arena, root)?;

        let mut out = String::new();
        comrak::format_commonmark(root, &options, &mut out)?;
        assert_eq!(
            out,
            "Before.\n\n\
             <table>\n\
             <tr>\n\
             <th>a</th>\n\
             <th align=\"right\">b</th>\n\
             </tr>\n\
             <tr>\n\
             <td><code>1</code><br>x</td>\n\
             <td align=\"right\"><img src=\"i.png\" alt=\"i\" /></td>\n\
             </tr>\n\
             </table>\n\n\
             After.\n"
        );
        Ok(())
    }
}
