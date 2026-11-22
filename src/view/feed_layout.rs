use super::hidden;
use crate::model::FeedLayout;

pub(super) fn switcher(layout: FeedLayout, back: &str) -> String {
    let back = hidden("back", back);
    let card_selected = matches!(layout, FeedLayout::Card);
    let compact_selected = matches!(layout, FeedLayout::Compact);
    format!(
        r#"<form class="feed-layout-options" method="post" action="/feeds/layout">
{back}<fieldset class="feed-layout-switch"><legend class="sr-only">Feed layout</legend>
<button type="submit" name="layout" value="card" aria-pressed="{card_selected}">Card</button>
<button type="submit" name="layout" value="compact" aria-pressed="{compact_selected}">Compact</button>
</fieldset></form>"#
    )
}
