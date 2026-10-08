//! The `<img>` tag of an online formula service: the service draws the TeX in the tag's URL, so a
//! page that cannot run MathJax still shows the formula.

use std::str::FromStr;

use super::page::escape_text;

/// The command line names of every [`MathService`], for help and error texts.
pub const SERVICE_NAMES: &str = "zhihu, codecogs, upmath, wordpress";

/// The bytes other than ASCII letters and digits that Python's `urllib.parse.quote` keeps.
const URL_SAFE_BYTES: &[u8] = b"_.-~/";

/// An online service that draws the TeX in a GET URL as an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathService {
    /// zhihu.com's equation service, SVG; zhihu's editor takes its `<img>` as an equation.
    Zhihu,
    /// latex.codecogs.com, SVG.
    Codecogs,
    /// i.upmath.me, SVG.
    Upmath,
    /// s0.wp.com's `latex.php`, PNG.
    Wordpress,
}

impl FromStr for MathService {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, String> {
        match name {
            "zhihu" => Ok(Self::Zhihu),
            "codecogs" => Ok(Self::Codecogs),
            "upmath" => Ok(Self::Upmath),
            "wordpress" => Ok(Self::Wordpress),
            _ => Err(format!(
                "unknown math service: {name}; one of: {SERVICE_NAMES}"
            )),
        }
    }
}

/// The URL at which `service` draws `tex`, in the display style when `display` is set.
pub fn math_url(service: MathService, tex: &str, display: bool) -> String {
    let tex = service_tex(service, tex, display);
    url_of(service, &tex)
}

/// The `<img>` tag whose image `service` draws from `tex`, in the display style when `display` is
/// set; the `alt` holds the TeX.
///
/// The zhihu tag is k3down2's `tex_to_zhihu`: `class="ee_img tr_noresize" eeimg="1"` makes zhihu's
/// editor take it as an equation, and the `alt` is not HTML-escaped beyond the `>` that
/// `zhihu_compatible` writes as `\gt`.
pub fn math_img_tag(service: MathService, tex: &str, display: bool) -> String {
    let tex = service_tex(service, tex, display);
    let url = url_of(service, &tex);
    if service == MathService::Zhihu {
        return format!(
            "<img src=\"{url}\" alt=\"{tex}\" class=\"ee_img tr_noresize\" eeimg=\"1\">"
        );
    }
    let alt = escape_text(&tex).replace('"', "&quot;");
    format!("<img src=\"{url}\" alt=\"{alt}\">")
}

/// `tex` as `service` takes it: trimmed, on one line, and in the display style when `display` is
/// set, which a trailing `\\` means on zhihu and `\displaystyle` means on the other services.
fn service_tex(service: MathService, tex: &str, display: bool) -> String {
    if service == MathService::Zhihu {
        let tex = zhihu_compatible(tex);
        return if display { format!("{tex}\\\\") } else { tex };
    }
    let tex = tex.trim().replace('\n', " ");
    if display {
        format!("\\displaystyle {tex}")
    } else {
        tex
    }
}

/// The URL at which `service` draws `tex`, which `service_tex` has already prepared.
fn url_of(service: MathService, tex: &str) -> String {
    let tex = quote(tex);
    match service {
        MathService::Zhihu => format!("https://www.zhihu.com/equation?tex={tex}"),
        MathService::Codecogs => format!("https://latex.codecogs.com/svg.image?{tex}"),
        MathService::Upmath => format!("https://i.upmath.me/svg/{tex}"),
        MathService::Wordpress => {
            format!("https://s0.wp.com/latex.php?latex={tex}&bg=fff&fg=444444&s=2&c=20201002")
        }
    }
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
    use super::*;

    #[test]
    fn test_from_str() {
        let service = MathService::from_str("codecogs");
        assert_eq!(service, Ok(MathService::Codecogs));

        let error = MathService::from_str("mathjax");
        let expected_error =
            "unknown math service: mathjax; one of: zhihu, codecogs, upmath, wordpress";
        assert_eq!(error, Err(expected_error.to_string()));
    }

    /// Every service has its own URL; a display formula ends with `\\` on zhihu and starts with
    /// `\displaystyle` elsewhere; the TeX is trimmed and put on one line.
    #[test]
    fn test_math_url() {
        let zhihu = math_url(MathService::Zhihu, "x^2", true);
        assert_eq!(zhihu, "https://www.zhihu.com/equation?tex=x%5E2%5C%5C");

        let codecogs = math_url(MathService::Codecogs, "x^2", false);
        assert_eq!(codecogs, "https://latex.codecogs.com/svg.image?x%5E2");

        let upmath = math_url(MathService::Upmath, "\na\n= b\n", true);
        assert_eq!(
            upmath,
            "https://i.upmath.me/svg/%5Cdisplaystyle%20a%20%3D%20b"
        );

        let wordpress = math_url(MathService::Wordpress, "E=mc^2", false);
        let expected_wordpress =
            "https://s0.wp.com/latex.php?latex=E%3Dmc%5E2&bg=fff&fg=444444&s=2&c=20201002";
        assert_eq!(wordpress, expected_wordpress);
    }

    /// A tag other than zhihu's has the TeX as `alt`, HTML-escaped.
    #[test]
    fn test_math_img_tag() {
        let tag = math_img_tag(MathService::Codecogs, "a<b & \"c\"", false);
        let expected_tag = r#"<img src="https://latex.codecogs.com/svg.image?a%3Cb%20%26%20%22c%22" alt="a&lt;b &amp; &quot;c&quot;">"#;
        assert_eq!(tag, expected_tag);
    }

    /// The zhihu tag is k3down2's, except that a newline inside the TeX becomes a space and a space
    /// separates `\gt` from a letter.
    #[test]
    fn test_zhihu_tag() {
        let k3down2_tag = math_img_tag(MathService::Zhihu, "x>1, \\>, a_b~/.-", false);
        let expected_k3down2_tag = r#"<img src="https://www.zhihu.com/equation?tex=x%5Cgt1%2C%20%5C%3E%2C%20a_b~/.-" alt="x\gt1, \>, a_b~/.-" class="ee_img tr_noresize" eeimg="1">"#;
        assert_eq!(k3down2_tag, expected_k3down2_tag);

        let display_tag = math_img_tag(MathService::Zhihu, "\n\\text{中}\n= y\n", true);
        let expected_display_tag = r#"<img src="https://www.zhihu.com/equation?tex=%5Ctext%7B%E4%B8%AD%7D%20%3D%20y%5C%5C" alt="\text{中} = y\\" class="ee_img tr_noresize" eeimg="1">"#;
        assert_eq!(display_tag, expected_display_tag);

        let letter_tag = math_img_tag(MathService::Zhihu, "x>y", false);
        let expected_letter_tag = r#"<img src="https://www.zhihu.com/equation?tex=x%5Cgt%20y" alt="x\gt y" class="ee_img tr_noresize" eeimg="1">"#;
        assert_eq!(letter_tag, expected_letter_tag);
    }
}
