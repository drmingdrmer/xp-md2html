//! `--preset`: the actions of one md2zhihu platform, from `platform_feature_dict` in md2zhihu's
//! `platform/__init__.py`.

use std::str::FromStr;

use super::code_to_image;
use super::Action;
use crate::render::math_img::MathService;

/// The names that `--preset` takes, as error messages list them.
pub const PRESET_NAMES: &str =
    "zhihu, github, wechat, weibo, simple, minimal_mistake, hugo, transparent";

/// An md2zhihu platform, named as md2zhihu's `--platform` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// zhihu.com: formulas as zhihu's equation images, tables as HTML, diagrams as PNGs.
    Zhihu,
    /// github.com: a `$$` formula in a list item on one line, graphviz diagrams as PNGs.
    Github,
    /// WeChat: what zhihu gets, and every code block as a PNG.
    Wechat,
    /// Weibo: tables and code as PNGs, inline formulas as text, and no lists or quotes.
    Weibo,
    /// Every table, formula and code block as a PNG.
    Simple,
    /// The jekyll theme minimal mistakes: diagrams as PNGs.
    MinimalMistake,
    /// Hugo with its passthrough extension: what minimal_mistake gets, and each formula on one line
    /// between Hugo's math delimiters.
    Hugo,
    /// Nothing but the steps that every preset runs.
    Transparent,
}

impl FromStr for Preset {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, String> {
        match name {
            "zhihu" => Ok(Self::Zhihu),
            "github" => Ok(Self::Github),
            "wechat" => Ok(Self::Wechat),
            "weibo" => Ok(Self::Weibo),
            "simple" => Ok(Self::Simple),
            "minimal_mistake" => Ok(Self::MinimalMistake),
            "hugo" => Ok(Self::Hugo),
            "transparent" => Ok(Self::Transparent),
            _ => Err(format!("unknown preset: {name}; one of: {PRESET_NAMES}")),
        }
    }
}

impl Preset {
    /// Whether this platform links bare URLs by default; website themes keep them as text.
    pub fn autolink(self) -> bool {
        let website = self == Self::Hugo || self == Self::MinimalMistake;
        !website
    }

    /// The name of the platform, as `--preset` and md2zhihu's `--platform` write it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Zhihu => "zhihu",
            Self::Github => "github",
            Self::Wechat => "wechat",
            Self::Weibo => "weibo",
            Self::Simple => "simple",
            Self::MinimalMistake => "minimal_mistake",
            Self::Hugo => "hugo",
            Self::Transparent => "transparent",
        }
    }

    /// The actions that give md2zhihu's result for this platform, in order.
    ///
    /// md2zhihu joins the `$$` formulas that blank lines split, embeds the `.md` images, drops the
    /// front matter, copies the local images and linked files, and lists the references on every
    /// platform. These steps run before the conversions:
    /// - `image-to-asset` would take an image that a conversion creates for a local image of the input.
    /// - `link-to-asset` would miss a link in a table that `table-to-html` turns into HTML.
    /// - No conversion changes the reference list, except weibo's `flatten-lists`, which flattens it
    ///   as md2zhihu writes it for Weibo: without a `<p>` in a `<li>`.
    pub fn actions(self) -> Vec<Action> {
        self.actions_with_code_widths(code_to_image::CodeWidths::default())
    }

    /// The platform's actions, with code image widths set before conversion.
    pub fn actions_with_code_widths(self, widths: code_to_image::CodeWidths) -> Vec<Action> {
        let mut actions = vec![
            Action::JoinMathBlock,
            Action::EmbedMarkdown,
            Action::DropFrontMatter,
            Action::ImageToAsset,
            Action::LinkToAsset,
            Action::AppendReferenceList,
        ];
        actions.extend(self.conversions(widths));
        actions
    }

    /// The actions of md2zhihu's converters for this platform.
    fn conversions(self, widths: code_to_image::CodeWidths) -> Vec<Action> {
        let zhihu_math = Action::MathToImgTag {
            service: MathService::Zhihu,
        };
        let code = Action::CodeToImage { widths };
        match self {
            Self::Zhihu => vec![
                zhihu_math,
                Action::TableToHtml,
                Action::MermaidToImage,
                Action::GraphvizToImage,
            ],
            Self::Github => vec![Action::MathBlockToOneLine, Action::GraphvizToImage],
            Self::Wechat => vec![
                zhihu_math,
                Action::TableToHtml,
                Action::MermaidToImage,
                Action::GraphvizToImage,
                code,
            ],
            // `math-to-img-tag` would take the inline formulas too, so they become text first.
            Self::Weibo => vec![
                Action::TableToImage,
                Action::MathInlineToText,
                zhihu_math,
                Action::MermaidToImage,
                Action::GraphvizToImage,
                code,
                Action::CodespanToText,
                Action::FlattenLists,
            ],
            Self::Simple => vec![
                Action::TableToImage,
                Action::MathToImage { service: None },
                Action::MermaidToImage,
                Action::GraphvizToImage,
                code,
                Action::CodespanToText,
            ],
            Self::MinimalMistake => vec![Action::MermaidToImage, Action::GraphvizToImage],
            Self::Hugo => vec![
                Action::MermaidToImage,
                Action::GraphvizToImage,
                Action::MathToHugo,
            ],
            Self::Transparent => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_from_str() {
        let preset = Preset::from_str("minimal_mistake");
        assert_eq!(preset, Ok(Preset::MinimalMistake));

        let error = Preset::from_str("medium");
        let expected_error = "unknown preset: medium; \
                              one of: zhihu, github, wechat, weibo, simple, minimal_mistake, hugo, transparent";
        assert_eq!(error, Err(expected_error.to_string()));
    }

    /// `--preset` takes the name of every preset.
    #[test]
    fn test_name() {
        let presets = [
            Preset::Zhihu,
            Preset::Github,
            Preset::Wechat,
            Preset::Weibo,
            Preset::Simple,
            Preset::MinimalMistake,
            Preset::Hugo,
            Preset::Transparent,
        ];
        let mut names = Vec::new();
        for preset in presets {
            let parsed = Preset::from_str(preset.name());
            assert_eq!(parsed, Ok(preset));
            names.push(preset.name());
        }
        assert_eq!(names.join(", "), PRESET_NAMES);
    }

    #[test]
    fn test_actions() {
        let zhihu = Preset::Zhihu.actions();
        let expected_zhihu = vec![
            Action::JoinMathBlock,
            Action::EmbedMarkdown,
            Action::DropFrontMatter,
            Action::ImageToAsset,
            Action::LinkToAsset,
            Action::AppendReferenceList,
            Action::MathToImgTag {
                service: MathService::Zhihu,
            },
            Action::TableToHtml,
            Action::MermaidToImage,
            Action::GraphvizToImage,
        ];
        assert_eq!(zhihu, expected_zhihu);

        let transparent = Preset::Transparent.actions();
        let expected_transparent = vec![
            Action::JoinMathBlock,
            Action::EmbedMarkdown,
            Action::DropFrontMatter,
            Action::ImageToAsset,
            Action::LinkToAsset,
            Action::AppendReferenceList,
        ];
        assert_eq!(transparent, expected_transparent);
    }
}
