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
    IntlEpisode(u64),
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
            return Ok(Self::Bvid(trimmed.to_owned()));
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
        if trimmed.chars().all(|ch| ch.is_ascii_digit()) {
            return parse_number(trimmed, "aid").map(Self::Aid);
        }

        Err(Error::InvalidInput(trimmed.to_owned()))
    }

    pub fn aid_hint(&self) -> Result<Option<u64>> {
        match self {
            Self::Aid(aid) => Ok(Some(*aid)),
            Self::Bvid(bvid) => bv::decode(bvid).map(Some),
            Self::Episode(_) | Self::Season(_) | Self::Media(_) | Self::IntlEpisode(_) => Ok(None),
        }
    }
}

fn parse_url(raw: &str) -> Result<Input> {
    let url = Url::parse(raw)?;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let path = url.path();

    if host == "b23.tv" {
        return Err(Error::Unsupported(
            "short-link redirect resolution is not in the first PR slice".to_owned(),
        ));
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
            return Ok(Input::Bvid(segment.to_owned()));
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

fn query_number(url: &Url, key: &str) -> Result<Option<u64>> {
    url.query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| parse_number(value.as_ref(), key))
        .transpose()
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
        Ok(())
    }
}
