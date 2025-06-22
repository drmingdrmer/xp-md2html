use std::io::Read;
use std::io::{self};

fn main() -> anyhow::Result<()> {
    let mut md = String::new();
    io::stdin().read_to_string(&mut md)?;

    println!(
        "{}",
        markdown::to_html_with_options(&md, &markdown::Options::gfm())
            .map_err(|e| anyhow::anyhow!("{}", e.to_string()))?
    );

    Ok(())
}
