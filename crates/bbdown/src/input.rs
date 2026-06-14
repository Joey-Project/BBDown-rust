use crate::bv;
use crate::{Error, Result};
use url::Url;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Input {
    Aid(u64),
    Bvid(String),
    Episode(u64),
    Season(u64),
    Media(u64),
    CheeseEpisode(u64),
    CheeseSeason(u64),
    SpaceVideos(u64),
    FavoriteList {
        media_id: Option<u64>,
        owner_mid: Option<u64>,
    },
    CollectionList(u64),
    SeriesList(u64),
    SpaceCollectionList {
        list_id: u64,
        owner_mid: u64,
    },
    SpaceSeriesList {
        list_id: u64,
        owner_mid: u64,
    },
    History,
    IntlEpisode(u64),
    ShortLink(String),
}

impl Input {
    pub fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(Error::InvalidInput("empty input".to_owned()));
        }

        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            return parse_url(trimmed);
        }

        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("bv") {
            return parse_bvid(trimmed);
        }
        if let Some(id) = lower.strip_prefix("av") {
            return parse_number(id, "av").map(Self::Aid);
        }
        if let Some(id) = lower.strip_prefix("ep") {
            return parse_number(id, "ep").map(Self::Episode);
        }
        if let Some(id) = lower.strip_prefix("ss") {
            return parse_number(id, "ss").map(Self::Season);
        }
        if let Some(id) = lower.strip_prefix("md") {
            return parse_number(id, "md").map(Self::Media);
        }
        if let Some(id) = lower.strip_prefix("cheese/ep") {
            return parse_number(id, "cheese ep").map(Self::CheeseEpisode);
        }
        if let Some(id) = lower.strip_prefix("cheese/ss") {
            return parse_number(id, "cheese season").map(Self::CheeseSeason);
        }
        if let Some(id) = lower.strip_prefix("mid") {
            return parse_number(id, "space mid").map(Self::SpaceVideos);
        }
        if let Some(id) = lower.strip_prefix("fav") {
            return parse_number(id, "favorite list").map(|media_id| Self::FavoriteList {
                media_id: Some(media_id),
                owner_mid: None,
            });
        }
        if let Some(id) = lower.strip_prefix("collection") {
            return parse_number(id, "collection list").map(Self::CollectionList);
        }
        if let Some(id) = lower.strip_prefix("series") {
            return parse_number(id, "series list").map(Self::SeriesList);
        }
        if lower == "history" {
            return Ok(Self::History);
        }
        if trimmed.chars().all(|ch| ch.is_ascii_digit()) {
            return parse_number(trimmed, "aid").map(Self::Aid);
        }

        Err(Error::InvalidInput(trimmed.to_owned()))
    }

    pub fn aid_hint(&self) -> Result<Option<u64>> {
        match self {
            Self::Aid(aid) => Ok(Some(*aid)),
            Self::Bvid(bvid) => bv::decode(bvid).map(Some),
            Self::Episode(_)
            | Self::Season(_)
            | Self::Media(_)
            | Self::CheeseEpisode(_)
            | Self::CheeseSeason(_)
            | Self::SpaceVideos(_)
            | Self::FavoriteList { .. }
            | Self::CollectionList(_)
            | Self::SeriesList(_)
            | Self::SpaceCollectionList { .. }
            | Self::SpaceSeriesList { .. }
            | Self::History
            | Self::IntlEpisode(_)
            | Self::ShortLink(_) => Ok(None),
        }
    }
}

fn parse_url(raw: &str) -> Result<Input> {
    let url = Url::parse(raw)?;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let path = url.path();

    if host == "b23.tv" {
        return Ok(Input::ShortLink(raw.to_owned()));
    }

    if host.ends_with("bilibili.tv") {
        let segments: Vec<_> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        if segments.len() >= 4 && segments[1] == "play" {
            return parse_number(segments[3], "intl episode").map(Input::IntlEpisode);
        }
    }

    if host == "space.bilibili.com"
        && let Some(input) = parse_space_url(&url)?
    {
        return Ok(input);
    }
    if parse_history_url(&url) {
        return Ok(Input::History);
    }

    if path.contains("/cheese/") {
        for segment in path.split('/').filter(|segment| !segment.is_empty()) {
            let lower = segment.to_ascii_lowercase();
            if let Some(id) = lower.strip_prefix("ep") {
                return parse_number(id, "cheese ep").map(Input::CheeseEpisode);
            }
            if let Some(id) = lower.strip_prefix("ss") {
                return parse_number(id, "cheese season").map(Input::CheeseSeason);
            }
        }
    }

    if let Some(input) = parse_medialist_url(&url)? {
        return Ok(input);
    }
    if let Some(input) = parse_list_url(&url)? {
        return Ok(input);
    }

    if let Some(value) = query_number(&url, "ep_id")? {
        return Ok(Input::Episode(value));
    }
    if let Some(value) = query_number(&url, "season_id")? {
        return Ok(Input::Season(value));
    }

    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        let lower = segment.to_ascii_lowercase();
        if let Some(id) = lower.strip_prefix("av") {
            return parse_number(id, "av").map(Input::Aid);
        }
        if lower.starts_with("bv") {
            return parse_bvid(segment);
        }
        if let Some(id) = lower.strip_prefix("ep") {
            return parse_number(id, "ep").map(Input::Episode);
        }
        if let Some(id) = lower.strip_prefix("ss") {
            return parse_number(id, "ss").map(Input::Season);
        }
        if let Some(id) = lower.strip_prefix("md") {
            return parse_number(id, "md").map(Input::Media);
        }
    }

    Err(Error::InvalidInput(raw.to_owned()))
}

fn parse_history_url(url: &Url) -> bool {
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    if host != "www.bilibili.com" && host != "bilibili.com" {
        return false;
    }
    let path_segments = url
        .path_segments()
        .map(std::iter::Iterator::collect::<Vec<_>>)
        .unwrap_or_default();
    path_segments.as_slice() == ["account", "history"]
}

fn parse_space_url(url: &Url) -> Result<Option<Input>> {
    let path_segments = url
        .path_segments()
        .map(std::iter::Iterator::collect::<Vec<_>>)
        .unwrap_or_default();
    let owner_mid = path_segments
        .first()
        .filter(|segment| segment.chars().all(|ch| ch.is_ascii_digit()))
        .map(|segment| parse_number(segment, "space mid"))
        .transpose()?;

    if path_segments.contains(&"favlist") {
        let media_id = query_number(url, "fid")?;
        return Ok(Some(Input::FavoriteList {
            media_id,
            owner_mid,
        }));
    }

    if path_segments.contains(&"channel")
        && path_segments
            .iter()
            .any(|segment| segment.starts_with("collectiondetail"))
    {
        let Some(list_id) = query_number(url, "sid")? else {
            return Ok(None);
        };
        return Ok(Some(match owner_mid {
            Some(owner_mid) => Input::SpaceCollectionList { list_id, owner_mid },
            None => Input::CollectionList(list_id),
        }));
    }
    if path_segments.contains(&"channel")
        && path_segments
            .iter()
            .any(|segment| segment.starts_with("seriesdetail"))
    {
        let Some(list_id) = query_number(url, "sid")? else {
            return Ok(None);
        };
        return Ok(Some(match owner_mid {
            Some(owner_mid) => Input::SpaceSeriesList { list_id, owner_mid },
            None => Input::SeriesList(list_id),
        }));
    }
    if let Some(list_index) = path_segments.iter().position(|segment| *segment == "lists")
        && let Some(id) = path_segments.get(list_index + 1)
    {
        let list_id = parse_number(id, "space list")?;
        return Ok(
            owner_mid.map(|owner_mid| match query_value(url, "type").as_deref() {
                Some("series") => Input::SpaceSeriesList { list_id, owner_mid },
                _ => Input::SpaceCollectionList { list_id, owner_mid },
            }),
        );
    }

    Ok(owner_mid.map(Input::SpaceVideos))
}

fn parse_medialist_url(url: &Url) -> Result<Option<Input>> {
    if !url.path().contains("/medialist/") {
        return Ok(None);
    }
    for segment in url.path_segments().into_iter().flatten() {
        let lower = segment.to_ascii_lowercase();
        if let Some(id) = lower.strip_prefix("ml") {
            return parse_number(id, "favorite list").map(|media_id| {
                Some(Input::FavoriteList {
                    media_id: Some(media_id),
                    owner_mid: None,
                })
            });
        }
    }
    let Some(business_id) = query_number(url, "business_id")? else {
        return Ok(None);
    };
    Ok(match query_value(url, "business").as_deref() {
        Some("space_series") => Some(Input::SeriesList(business_id)),
        Some("space_collection") | None => Some(Input::CollectionList(business_id)),
        Some(_) => None,
    })
}

fn parse_list_url(url: &Url) -> Result<Option<Input>> {
    let path_segments = url
        .path_segments()
        .map(std::iter::Iterator::collect::<Vec<_>>)
        .unwrap_or_default();
    if path_segments.first() != Some(&"list") {
        return Ok(None);
    }
    let Some(list_segment) = path_segments.get(1) else {
        return Ok(None);
    };
    let lower_list_segment = list_segment.to_ascii_lowercase();
    if let Some(id) = lower_list_segment.strip_prefix("ml") {
        return parse_number(id, "favorite list").map(|media_id| {
            Some(Input::FavoriteList {
                media_id: Some(media_id),
                owner_mid: None,
            })
        });
    }
    let owner_mid = parse_number(list_segment, "space mid")?;
    let Some(list_id) = query_number(url, "sid")? else {
        return Ok(None);
    };
    Ok(Some(match query_value(url, "type").as_deref() {
        Some("series") => Input::SpaceSeriesList { list_id, owner_mid },
        _ => Input::SpaceCollectionList { list_id, owner_mid },
    }))
}

fn parse_bvid(text: &str) -> Result<Input> {
    bv::decode(text)?;
    Ok(Input::Bvid(text.to_owned()))
}

fn query_number(url: &Url, key: &str) -> Result<Option<u64>> {
    url.query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| parse_number(value.as_ref(), key))
        .transpose()
}

fn query_value(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
}

fn parse_number(text: &str, label: &str) -> Result<u64> {
    text.parse::<u64>()
        .map_err(|_| Error::InvalidInput(format!("invalid {label} id `{text}`")))
}

#[cfg(test)]
mod tests {
    use crate::Input;

    #[test]
    fn parses_common_inputs() -> anyhow::Result<()> {
        assert_eq!(Input::parse("av170001")?, Input::Aid(170_001));
        assert_eq!(
            Input::parse("BV1qt4y1X7TW")?,
            Input::Bvid("BV1qt4y1X7TW".to_owned())
        );
        assert_eq!(
            Input::parse("https://www.bilibili.com/bangumi/play/ep359333")?,
            Input::Episode(359_333)
        );
        assert_eq!(
            Input::parse("https://www.bilibili.com/bangumi/play/ss28276")?,
            Input::Season(28_276)
        );
        assert_eq!(
            Input::parse("https://www.bilibili.com/bangumi/media/md28230188/")?,
            Input::Media(28_230_188)
        );
        assert_eq!(
            Input::parse("https://www.bilibili.tv/en/play/34613/341736")?,
            Input::IntlEpisode(341_736)
        );
        assert_eq!(
            Input::parse("https://www.bilibili.com/cheese/play/ep101")?,
            Input::CheeseEpisode(101)
        );
        assert_eq!(Input::parse("cheese/ss202")?, Input::CheeseSeason(202));
        assert_eq!(
            Input::parse("https://space.bilibili.com/123/favlist?fid=456")?,
            Input::FavoriteList {
                media_id: Some(456),
                owner_mid: Some(123),
            }
        );
        assert_eq!(
            Input::parse("https://space.bilibili.com/123/channel/collectiondetail?sid=456")?,
            Input::SpaceCollectionList {
                list_id: 456,
                owner_mid: 123,
            }
        );
        assert_eq!(
            Input::parse("https://space.bilibili.com/123/lists/456?type=series")?,
            Input::SpaceSeriesList {
                list_id: 456,
                owner_mid: 123,
            }
        );
        assert_eq!(
            Input::parse("https://www.bilibili.com/medialist/detail/ml1103407912")?,
            Input::FavoriteList {
                media_id: Some(1_103_407_912),
                owner_mid: None,
            }
        );
        assert_eq!(
            Input::parse("https://www.bilibili.com/list/ml1103407912")?,
            Input::FavoriteList {
                media_id: Some(1_103_407_912),
                owner_mid: None,
            }
        );
        assert_eq!(
            Input::parse("https://www.bilibili.com/list/1958703906?sid=547718&type=series")?,
            Input::SpaceSeriesList {
                list_id: 547_718,
                owner_mid: 1_958_703_906,
            }
        );
        assert_eq!(
            Input::parse("https://www.bilibili.com/list/1958703906?sid=547718")?,
            Input::SpaceCollectionList {
                list_id: 547_718,
                owner_mid: 1_958_703_906,
            }
        );
        assert_eq!(Input::parse("history")?, Input::History);
        assert_eq!(
            Input::parse("https://www.bilibili.com/account/history")?,
            Input::History
        );
        assert_eq!(
            Input::parse("https://space.bilibili.com/123")?,
            Input::SpaceVideos(123)
        );
        assert_eq!(
            Input::parse("https://b23.tv/example")?,
            Input::ShortLink("https://b23.tv/example".to_owned())
        );
        assert!(Input::parse("bvideo").is_err());
        assert!(Input::parse("bv1qt4y1X7TW").is_err());
        Ok(())
    }
}
