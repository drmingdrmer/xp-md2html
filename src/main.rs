use std::io::Read;
use std::io::{self};

fn main() -> anyhow::Result<()> {
    let mut md = String::new();
    io::stdin().read_to_string(&mut md)?;

    // Keep raw HTML: md2zhihu writes tables and formula images as HTML.
    let mut options = markdown::Options::gfm();
    options.compile.allow_dangerous_html = true;

    println!(
        "{}",
        markdown::to_html_with_options(&md, &options)
            .map_err(|e| anyhow::anyhow!("{}", e.to_string()))?
    );

    Ok(())
}
