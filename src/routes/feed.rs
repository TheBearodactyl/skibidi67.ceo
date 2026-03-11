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

fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;
    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{} B", bytes)
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
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
        .map(|e| e.value().clone())
        .collect();

    items.sort_unstable_by_key(|v| std::cmp::Reverse(v.uploaded_at));
    items.truncate(50);

    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"
     xmlns:media="http://search.yahoo.com/mrss/"
     xmlns:dc="http://purl.org/dc/elements/1.1/"
     xmlns:atom="http://www.w3.org/2005/Atom">
<channel>
<title>{title}</title>
<link>{base_url}/ui</link>
<description>{description}</description>
<language>en-us</language>
<atom:link href="{base_url}/ui/{slug}.rss" rel="self" type="application/rss+xml"/>
"#,
        title = xml_escape(title),
        base_url = xml_escape(&site.base_url),
        description = xml_escape(&format!("Latest uploads on {}", site.site_host)),
        slug = match filter {
            Some("video/") => "videos",
            Some("audio/") => "audio",
            Some("image/") => "images",
            Some("text/") => "text",
            _ => "all",
        },
    );

    for v in &items {
        let prefix = media_type_for(&v.content_type);
        let item_url = format!("{}/ui/{}/{}", site.base_url, prefix, v.id);
        let file_url = format!("{}/{}/{}/file", site.base_url, prefix, v.id);
        let pub_date = v
            .uploaded_at
            .format("%a, %d %b %Y %H:%M:%S +0000")
            .to_string();

        // ---- description HTML block ----------------------------------------
        let mut desc = String::new();

        // Embed block depending on media type
        if v.content_type.starts_with("video/") {
            desc.push_str(&format!(
                r#"<p><video controls preload="metadata" style="max-width:100%;"><source src="{file_url}" type="{ct}"></video></p>"#,
                file_url = xml_escape(&file_url),
                ct = xml_escape(&v.content_type),
            ));
        } else if v.content_type.starts_with("audio/") {
            desc.push_str(&format!(
                r#"<p><audio controls preload="metadata" style="width:100%;"><source src="{file_url}" type="{ct}"></audio></p>"#,
                file_url = xml_escape(&file_url),
                ct = xml_escape(&v.content_type),
            ));
        } else if v.content_type.starts_with("image/") {
            desc.push_str(&format!(
                r#"<p><img src="{file_url}" alt="{title}" style="max-width:100%;height:auto;"></p>"#,
                file_url = xml_escape(&file_url),
                title = xml_escape(&v.title),
            ));
        }

        desc.push_str(&format!(
            r#"<p>Uploaded by <strong>{uploader}</strong> on {date}</p>"#,
            uploader = xml_escape(&v.uploaded_by_name),
            date = v.uploaded_at.format("%Y-%m-%d"),
        ));

        desc.push_str(&format!(
            r#"<p>Type: <code>{ct}</code> &nbsp;|&nbsp; Size: {size}</p>"#,
            ct = xml_escape(&v.content_type),
            size = human_size(v.size_bytes),
        ));

        if let (Some(name), Some(link)) = (&v.source_name, &v.source_link) {
            if !name.is_empty() && !link.is_empty() {
                desc.push_str(&format!(
                    r#"<p>Source: <a href="{link}">{name}</a></p>"#,
                    link = xml_escape(link),
                    name = xml_escape(name),
                ));
            } else if !name.is_empty() {
                desc.push_str(&format!(
                    r#"<p>Source: {name}</p>"#,
                    name = xml_escape(name),
                ));
            }
        }

        desc.push_str(&format!(
            r#"<p><a href="{url}">View on {host}</a></p>"#,
            url = xml_escape(&item_url),
            host = xml_escape(&site.site_host),
        ));

        // ---- enclosure / media:content for feed readers that support them ----
        let enclosure_xml =
            if v.content_type.starts_with("video/") || v.content_type.starts_with("audio/") {
                format!(
                    r#"<enclosure url="{url}" length="{len}" type="{ct}"/>"#,
                    url = xml_escape(&file_url),
                    len = v.size_bytes,
                    ct = xml_escape(&v.content_type),
                )
            } else {
                String::new()
            };

        let media_content_xml = if v.content_type.starts_with("image/") {
            format!(
                r#"<media:content url="{url}" type="{ct}" medium="image"/>"#,
                url = xml_escape(&file_url),
                ct = xml_escape(&v.content_type),
            )
        } else if v.content_type.starts_with("video/") {
            format!(
                r#"<media:content url="{url}" type="{ct}" medium="video" fileSize="{len}"/>"#,
                url = xml_escape(&file_url),
                ct = xml_escape(&v.content_type),
                len = v.size_bytes,
            )
        } else if v.content_type.starts_with("audio/") {
            format!(
                r#"<media:content url="{url}" type="{ct}" medium="audio" fileSize="{len}"/>"#,
                url = xml_escape(&file_url),
                ct = xml_escape(&v.content_type),
                len = v.size_bytes,
            )
        } else {
            String::new()
        };

        xml.push_str(&format!(
            r#"<item>
<title>{title}</title>
<link>{link}</link>
<guid isPermaLink="true">{link}</guid>
<pubDate>{pub_date}</pubDate>
<dc:creator>{creator}</dc:creator>
<description><![CDATA[{desc}]]></description>
{enclosure}{media_content}</item>
"#,
            title = xml_escape(&v.title),
            link = xml_escape(&item_url),
            pub_date = pub_date,
            creator = xml_escape(&v.uploaded_by_name),
            desc = desc,
            enclosure = enclosure_xml,
            media_content = media_content_xml,
        ));
    }

    xml.push_str("</channel>\n</rss>");

    (ContentType::new("application", "rss+xml"), xml)
}

#[get("/ui/all.rss")]
pub fn feed_all(state: &State<AppState>, site: SiteInfo) -> (ContentType, String) {
    build_feed(state.inner(), &site, "All Recent Uploads", None)
}

#[get("/ui/videos.rss")]
pub fn feed_videos(state: &State<AppState>, site: SiteInfo) -> (ContentType, String) {
    build_feed(state.inner(), &site, "Recent Videos", Some("video/"))
}

#[get("/ui/audio.rss")]
pub fn feed_audio(state: &State<AppState>, site: SiteInfo) -> (ContentType, String) {
    build_feed(state.inner(), &site, "Recent Audio", Some("audio/"))
}

#[get("/ui/images.rss")]
pub fn feed_images(state: &State<AppState>, site: SiteInfo) -> (ContentType, String) {
    build_feed(state.inner(), &site, "Recent Images", Some("image/"))
}

#[get("/ui/text.rss")]
pub fn feed_text(state: &State<AppState>, site: SiteInfo) -> (ContentType, String) {
    build_feed(state.inner(), &site, "Recent Text", Some("text/"))
}
