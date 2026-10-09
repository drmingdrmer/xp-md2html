//! `--preset`: the actions of one md2zhihu platform, from `platform_feature_dict` in md2zhihu's
//! `platform/__init__.py`.

use std::str::FromStr;

use super::code_to_image;
use super::Action;
use crate::render::math_img::MathService;

/// The names that `--preset` takes, as error messages list them.
pub const PRESET_NAMES: &str = "zhihu, github, wechat, weibo, simple, minimal_mistake, transparent";

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
            "transparent" => Ok(Self::Transparent),
            _ => Err(format!("unknown preset: {name}; one of: {PRESET_NAMES}")),
        }
    }
}

impl Preset {
    /// The actions that give md2zhihu's result for this platform, in order.
    ///
    /// md2zhihu embeds the `.md` images, drops the front matter, copies the local images and lists
    /// the references on every platform. These steps run before the conversions:
    /// - `image-to-asset` would take an image that a conversion creates for a local image of the input.
    /// - No conversion changes the reference list, except weibo's `flatten-lists`, which flattens it
    ///   as md2zhihu writes it for Weibo: without a `<p>` in a `<li>`.
    pub fn actions(self) -> Vec<Action> {
        let mut actions = vec![
            Action::EmbedMarkdown,
            Action::DropFrontMatter,
            Action::ImageToAsset,
            Action::AppendReferenceList,
        ];
        actions.extend(self.conversions());
        actions
    }

    /// The actions of md2zhihu's converters for this platform.
    fn conversions(self) -> Vec<Action> {
        let zhihu_math = Action::MathToImgTag {
            service: MathService::Zhihu,
        };
        let code = Action::CodeToImage {
            width: code_to_image::DEFAULT_WIDTH,
        };
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
                              one of: zhihu, github, wechat, weibo, simple, minimal_mistake, transparent";
        assert_eq!(error, Err(expected_error.to_string()));
    }

    #[test]
    fn test_actions() {
        let zhihu = Preset::Zhihu.actions();
        let expected_zhihu = vec![
            Action::EmbedMarkdown,
            Action::DropFrontMatter,
            Action::ImageToAsset,
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
            Action::EmbedMarkdown,
            Action::DropFrontMatter,
            Action::ImageToAsset,
            Action::AppendReferenceList,
        ];
        assert_eq!(transparent, expected_transparent);
    }
}
