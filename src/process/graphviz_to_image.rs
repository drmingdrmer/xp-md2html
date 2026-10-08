//! `graphviz-to-image`: render every ```` ```graphviz ```` block to a PNG and link the PNG where the block was.

use comrak::Arena;
use comrak::Node;

use super::mermaid_to_image::render;
use super::mermaid_to_image::replace_code_blocks;
use super::ActionContext;
use crate::render::graphviz::graphviz_to_svg;

/// Replace every ```` ```graphviz ```` block under `root` with a PNG that `ctx.renderer` renders
/// into `ctx.assets_dir`.
pub fn apply<'a>(arena: &'a Arena<'a>, root: Node<'a>, ctx: &ActionContext) -> anyhow::Result<()> {
    replace_code_blocks(arena, root, "graphviz", |source| {
        render(source, "graphviz", graphviz_to_svg, ctx)
    })
}
