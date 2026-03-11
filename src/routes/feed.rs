use {
    crate::{routes::ui::SiteInfo, state::AppState},
    rocket::{State, get, http::ContentType},
};

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn media_type_for(content_type: &str) -> &'static str {
    match content_type {
        t if t.starts_with("audio/") => "audio",
        t if t.starts_with("image/") => "image",
        t if t.starts_with("text/") => "text",
        _ => "videos",
    }
}

fn build_feed(
    state: &AppState,
    site: &SiteInfo,
    title: &str,
    filter: Option<&str>,
) -> (ContentType, String) {
    let mut items: Vec<_> = state
        .videos
        .iter()
        .filter(|e| {
            let v = e.value();
            !v.unlisted
                && !v.nsfw
                && filter
                    .map(|prefix| v.content_type.starts_with(prefix))
                    .unwrap_or(true)
        })
        .map(|e| {
            let v = e.value();
            (
                v.id.clone(),
                v.title.clone(),
                v.content_type.clone(),
                v.uploaded_by_name.clone(),
                v.uploaded_at,
            )
        })
        .collect();

    items.sort_unstable_by_key(|b| std::cmp::Reverse(b.4));
    items.truncate(50);

    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
<channel>
<title>{}</title>
<link>{}/ui</link>
<description>{}</description>
"#,
        xml_escape(title),
        xml_escape(&site.base_url),
        xml_escape(&format!("Latest uploads on {}", site.site_host)),
    );

    for (id, item_title, content_type, uploader, uploaded_at) in &items {
        let prefix = media_type_for(content_type);
        let link = format!("{}/ui/{}/{}", site.base_url, prefix, id);
        let pub_date = uploaded_at
            .format("%a, %d %b %Y %H:%M:%S +0000")
            .to_string();

        xml.push_str(&format!(
            r#"<item>
<title>{}</title>
<link>{}</link>
<guid isPermaLink="true">{}</guid>
<pubDate>{}</pubDate>
<description>Uploaded by {}</description>
</item>
"#,
            xml_escape(item_title),
            xml_escape(&link),
            xml_escape(&link),
            pub_date,
            xml_escape(uploader),
        ));
    }

    xml.push_str("</channel>\n</rss>");

    (ContentType::new("application", "rss+xml"), xml)
}

#[get("/feed")]
pub fn feed_all(state: &State<AppState>, site: SiteInfo) -> (ContentType, String) {
    build_feed(state.inner(), &site, "All Recent Uploads", None)
}

#[get("/feed/videos")]
pub fn feed_videos(state: &State<AppState>, site: SiteInfo) -> (ContentType, String) {
    build_feed(state.inner(), &site, "Recent Videos", Some("video/"))
}

#[get("/feed/audio")]
pub fn feed_audio(state: &State<AppState>, site: SiteInfo) -> (ContentType, String) {
    build_feed(state.inner(), &site, "Recent Audio", Some("audio/"))
}

#[get("/feed/images")]
pub fn feed_images(state: &State<AppState>, site: SiteInfo) -> (ContentType, String) {
    build_feed(state.inner(), &site, "Recent Images", Some("image/"))
}

#[get("/feed/text")]
pub fn feed_text(state: &State<AppState>, site: SiteInfo) -> (ContentType, String) {
    build_feed(state.inner(), &site, "Recent Text", Some("text/"))
}
