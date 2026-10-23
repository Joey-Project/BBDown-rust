use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use url::Url;

const BUNDLED_CATEGORY_CATALOG: &str = include_str!("category_catalog.json");
const CATEGORY_CATALOG_SCOPE: &str = "bundled_channel_menu_fallback";

/// A bundled snapshot of the official site's channel menu, including its hierarchy.
///
/// The menu is a navigation fallback, not a complete upstream taxonomy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CategoryCatalog {
    pub scope: String,
    pub captured_at: String,
    pub source_url: String,
    pub source_asset_url: String,
    pub source_asset_sha256: String,
    pub entries: Vec<CategoryCatalogEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CategoryCatalogEntry {
    pub name: String,
    pub url: String,
    pub channel_id: Option<u32>,
    pub sub_channel_id: Option<u32>,
    pub tid: Option<u32>,
    pub children: Vec<CategoryCatalogEntry>,
}

/// Loads and validates the bundled official-site channel-menu fallback snapshot.
pub fn category_catalog() -> Result<CategoryCatalog> {
    parse_category_catalog(BUNDLED_CATEGORY_CATALOG)
}

fn parse_category_catalog(raw: &str) -> Result<CategoryCatalog> {
    let catalog: CategoryCatalog = serde_json::from_str(raw)?;
    validate_catalog(&catalog)?;
    Ok(catalog)
}

fn validate_catalog(catalog: &CategoryCatalog) -> Result<()> {
    if catalog.scope != CATEGORY_CATALOG_SCOPE {
        return Err(invalid_catalog("scope"));
    }
    if catalog.captured_at.trim().is_empty() {
        return Err(invalid_catalog("captured_at"));
    }
    validate_source_url(&catalog.source_url)?;
    validate_asset_url(&catalog.source_asset_url)?;
    if catalog.source_asset_sha256.len() != 64
        || !catalog
            .source_asset_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(invalid_catalog("source_asset_sha256"));
    }
    validate_entries(&catalog.entries)
}

fn validate_source_url(value: &str) -> Result<()> {
    let parsed = parse_https_url(value, "source_url")?;
    if parsed.host_str() != Some("www.bilibili.com") {
        return Err(invalid_catalog("source_url"));
    }
    Ok(())
}

fn validate_asset_url(value: &str) -> Result<()> {
    let parsed = parse_https_url(value, "source_asset_url")?;
    let host = parsed
        .host_str()
        .ok_or_else(|| invalid_catalog("source_asset_url"))?;
    if host != "hdslb.com" && !host.ends_with(".hdslb.com") {
        return Err(invalid_catalog("source_asset_url"));
    }
    Ok(())
}

fn parse_https_url(value: &str, field: &'static str) -> Result<Url> {
    let parsed = Url::parse(value).map_err(|_| invalid_catalog(field))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || has_userinfo_separator(value)
    {
        return Err(invalid_catalog(field));
    }
    Ok(parsed)
}

fn has_userinfo_separator(value: &str) -> bool {
    value
        .split_once("://")
        .and_then(|(_, remainder)| remainder.split(['/', '?', '#']).next())
        .is_some_and(|authority| authority.contains('@'))
}

fn validate_entries(entries: &[CategoryCatalogEntry]) -> Result<()> {
    for entry in entries {
        if entry.name.trim().is_empty() {
            return Err(invalid_catalog("entry name"));
        }
        validate_navigation_url(&entry.url)?;
        validate_optional_id(entry.channel_id)?;
        validate_optional_id(entry.sub_channel_id)?;
        validate_optional_id(entry.tid)?;
        validate_entries(&entry.children)?;
    }
    Ok(())
}

fn validate_navigation_url(value: &str) -> Result<()> {
    let parsed = parse_https_url(value, "entry url")?;
    let host = parsed
        .host_str()
        .ok_or_else(|| invalid_catalog("entry url"))?;
    if host != "bilibili.com" && !host.ends_with(".bilibili.com") {
        return Err(invalid_catalog("entry url"));
    }
    Ok(())
}

fn validate_optional_id(value: Option<u32>) -> Result<()> {
    if value.is_some_and(|id| id == 0) {
        return Err(invalid_catalog("entry ID"));
    }
    Ok(())
}

fn invalid_catalog(field: &str) -> Error {
    Error::InvalidInput(format!("invalid bundled category catalog {field}"))
}

#[cfg(test)]
mod tests {
    use super::{CategoryCatalog, CategoryCatalogEntry, category_catalog, parse_category_catalog};
    use serde_json::{Value, json};

    #[test]
    fn bundled_catalog_preserves_official_menu_scope_provenance_and_hierarchy() -> anyhow::Result<()>
    {
        let catalog = category_catalog()?;
        assert_eq!(catalog.scope, "bundled_channel_menu_fallback");
        assert_eq!(catalog.captured_at, "2026-10-22T22:32:44.646206Z");
        assert_eq!(catalog.source_url, "https://www.bilibili.com/c/douga/");
        assert_eq!(
            catalog.source_asset_url,
            "https://s1.hdslb.com/bfs/static/shanks/channel/assets/index-bedd2c00.js"
        );
        assert_eq!(
            catalog.source_asset_sha256,
            "e9a6380a51acb054d8728db716a10a13818371a7834ba5f4f53a4b7872cfcbcb"
        );
        assert_eq!(catalog.entries.len(), 28);
        assert_eq!(
            catalog
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            [
                "番剧",
                "电影",
                "国创",
                "电视剧",
                "综艺",
                "纪录片",
                "动画",
                "游戏",
                "鬼畜",
                "音乐",
                "舞蹈",
                "影视",
                "娱乐",
                "知识",
                "科技",
                "资讯",
                "美食",
                "生活",
                "汽车",
                "时尚",
                "运动",
                "动物圈",
                "VLOG",
                "搞笑",
                "单机游戏",
                "虚拟UP主",
                "公益",
                "公开课"
            ]
        );
        assert_eq!(
            catalog
                .entries
                .iter()
                .map(|entry| entry.children.len())
                .sum::<usize>(),
            114
        );
        Ok(())
    }

    #[test]
    fn bundled_facts_keep_missing_ids_duplicates_and_navigation_fragments() -> anyhow::Result<()> {
        let catalog = category_catalog()?;
        let anime = &catalog.entries[0];
        assert_eq!(anime.name, "番剧");
        assert_eq!(anime.children.len(), 6);
        assert_eq!(anime.children[4].name, "新番时间表");
        assert_eq!(anime.children[4].tid, None);
        assert_eq!(anime.children[5].name, "番剧索引");
        assert_eq!(anime.children[5].tid, None);

        let animation = catalog
            .entries
            .iter()
            .find(|entry| entry.name == "动画")
            .ok_or_else(|| anyhow::anyhow!("animation menu entry must be present"))?;
        assert_eq!(animation.channel_id, Some(8));
        assert_eq!(animation.tid, Some(1));

        let vlog = catalog
            .entries
            .iter()
            .find(|entry| entry.name == "VLOG")
            .ok_or_else(|| anyhow::anyhow!("VLOG menu entry must be present"))?;
        assert!(vlog.url.ends_with("#/530003"));
        assert_eq!(vlog.tid, None);
        assert_eq!(catalog.entries[4].tid, None);

        for duplicate_tid in [17, 138, 246] {
            assert_eq!(count_tid(&catalog.entries, duplicate_tid), 2);
        }
        assert_eq!(count_missing_tid(&catalog.entries), 12);
        Ok(())
    }

    fn count_tid(entries: &[CategoryCatalogEntry], tid: u32) -> usize {
        entries
            .iter()
            .map(|entry| usize::from(entry.tid == Some(tid)) + count_tid(&entry.children, tid))
            .sum()
    }

    fn count_missing_tid(entries: &[CategoryCatalogEntry]) -> usize {
        entries
            .iter()
            .map(|entry| usize::from(entry.tid.is_none()) + count_missing_tid(&entry.children))
            .sum()
    }

    fn fixture() -> Value {
        json!({
            "scope": "bundled_channel_menu_fallback",
            "captured_at": "2026-10-22T22:32:44.646206Z",
            "source_url": "https://www.bilibili.com/c/douga/",
            "source_asset_url": "https://s1.hdslb.com/assets/channel.js",
            "source_asset_sha256": "a".repeat(64),
            "entries": [{
                "name": "Menu item",
                "url": "https://www.bilibili.com/v/douga/",
                "channel_id": 8,
                "sub_channel_id": null,
                "tid": 1,
                "children": []
            }]
        })
    }

    fn parse_fixture(value: &Value) -> anyhow::Result<CategoryCatalog> {
        Ok(parse_category_catalog(&value.to_string())?)
    }

    #[test]
    fn rejects_zero_ids_blank_names_and_untrusted_navigation_urls() {
        let mut zero_id = fixture();
        zero_id["entries"][0]["tid"] = json!(0);
        assert!(parse_fixture(&zero_id).is_err());

        let mut blank_name = fixture();
        blank_name["entries"][0]["name"] = json!("  ");
        assert!(parse_fixture(&blank_name).is_err());

        for url in [
            "http://www.bilibili.com/v/douga/",
            "https://www.bilibili.example/v/douga/",
            "https://user@www.bilibili.com/v/douga/",
        ] {
            let mut invalid_url = fixture();
            invalid_url["entries"][0]["url"] = json!(url);
            assert!(parse_fixture(&invalid_url).is_err());
        }
    }

    #[test]
    fn rejects_untrusted_source_asset_urls_and_malformed_hashes() {
        let mut wrong_asset_host = fixture();
        wrong_asset_host["source_asset_url"] = json!("https://assets.example/channel.js");
        assert!(parse_fixture(&wrong_asset_host).is_err());

        let mut wrong_asset_scheme = fixture();
        wrong_asset_scheme["source_asset_url"] = json!("http://s1.hdslb.com/channel.js");
        assert!(parse_fixture(&wrong_asset_scheme).is_err());

        let mut malformed_hash = fixture();
        malformed_hash["source_asset_sha256"] = json!("not-a-sha256");
        assert!(parse_fixture(&malformed_hash).is_err());
    }

    #[test]
    fn preserves_valid_navigation_query_and_fragment() -> anyhow::Result<()> {
        let mut value = fixture();
        value["entries"][0]["url"] = json!("https://love.bilibili.com/path?from=menu#/event");
        let catalog = parse_fixture(&value)?;
        assert_eq!(
            catalog.entries[0].url,
            "https://love.bilibili.com/path?from=menu#/event"
        );
        Ok(())
    }
}
