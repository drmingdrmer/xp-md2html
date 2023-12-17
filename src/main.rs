use std::io::{self, Read};

fn main() -> Result<(), String> {

    let mut md = String::new();
    io::stdin().read_to_string(&mut md).map_err(|e| e.to_string())?;

    println!(
        "{}",
        markdown::to_html_with_options(
            &md,
            &markdown::Options::gfm()
        )?
    );

    Ok(())
}
