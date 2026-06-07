use crate::{Error, Result};
use std::str::FromStr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Selection {
    Current,
    Latest,
    All,
    Episode(u64),
    Page(u32),
}

impl FromStr for Selection {
    type Err = Error;

    fn from_str(raw: &str) -> Result<Self> {
        let text = raw.trim();
        let lower = text.to_ascii_lowercase();
        match lower.as_str() {
            "current" => Ok(Self::Current),
            "latest" | "last" | "new" => Ok(Self::Latest),
            "all" => Ok(Self::All),
            _ => {
                if let Some(id) = lower.strip_prefix("episode:") {
                    return parse_u64(id, "episode").map(Self::Episode);
                }
                if let Some(page) = lower.strip_prefix("page:") {
                    return parse_u32(page, "page").map(Self::Page);
                }
                Err(Error::InvalidInput(format!("invalid selection `{raw}`")))
            }
        }
    }
}

fn parse_u64(text: &str, label: &str) -> Result<u64> {
    text.parse::<u64>()
        .map_err(|_| Error::InvalidInput(format!("invalid {label} selection `{text}`")))
}

fn parse_u32(text: &str, label: &str) -> Result<u32> {
    text.parse::<u32>()
        .map_err(|_| Error::InvalidInput(format!("invalid {label} selection `{text}`")))
}
