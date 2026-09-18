//! Portable HTML for copying Markdown responses into rich-text applications.
//!
//! Raw HTML is escaped, and destinations use the TUI's web-link policy. Link
//! destinations that cannot be linked remain visible as escaped text. The original
//! Markdown remains the plain-text clipboard representation. Images render only
//! their alt text so pasting cannot trigger remote image requests.

use pulldown_cmark::Event;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;

pub(crate) fn render_markdown(markdown: &str) -> String {
    let normalized = crate::markdown::unwrap_markdown_fences(markdown);
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let mut link_destinations = Vec::new();
    let events = Parser::new_ext(&normalized, options).filter_map(|event| {
        Some(match event {
            Event::Html(text) | Event::InlineHtml(text) => Event::Text(text),
            Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image) => return None,
            Event::Start(mut tag) => {
                if let Tag::Link { dest_url, .. } = &mut tag {
                    if let Some(destination) = crate::terminal_hyperlinks::web_destination(dest_url)
                    {
                        *dest_url = destination.into();
                        link_destinations.push(/*value*/ None);
                    } else {
                        link_destinations.push(Some(dest_url.clone()));
                        return None;
                    }
                }
                Event::Start(tag)
            }
            Event::End(TagEnd::Link) => match link_destinations.pop().flatten() {
                Some(destination) => Event::Text(format!(" ({destination})").into()),
                None => Event::End(TagEnd::Link),
            },
            event => event,
        })
    });
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events);
    html
}

#[cfg(test)]
#[path = "clipboard_html_tests.rs"]
mod tests;
