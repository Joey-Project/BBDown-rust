use crate::{Error, Result};
use std::str::FromStr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Selection {
    Current,
    Latest,
    All,
    Episode(u64),
    Page(u32),
    Indices(IndexSelection),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexSelection {
    selectors: Vec<IndexSelector>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexSelector {
    Index(u32),
    Range { start: u32, end: u32 },
}

impl IndexSelection {
    pub fn new(selectors: impl Into<Vec<IndexSelector>>) -> Result<Self> {
        let selectors = selectors.into();
        if selectors.is_empty() {
            return Err(Error::InvalidInput(
                "index selection must contain at least one selector".to_owned(),
            ));
        }
        for selector in &selectors {
            validate_index_selector(*selector)?;
        }
        Ok(Self { selectors })
    }

    pub fn single(index: u32) -> Result<Self> {
        Self::new([IndexSelector::Index(index)])
    }

    pub fn range(start: u32, end: u32) -> Result<Self> {
        Self::new([IndexSelector::Range { start, end }])
    }

    #[must_use]
    pub fn selectors(&self) -> &[IndexSelector] {
        &self.selectors
    }

    #[must_use]
    pub fn max_index(&self) -> u32 {
        self.selectors
            .iter()
            .map(|selector| match selector {
                IndexSelector::Index(index) => *index,
                IndexSelector::Range { end, .. } => *end,
            })
            .max()
            .unwrap_or(0)
    }

    #[must_use]
    pub fn contains(&self, index: u32) -> bool {
        self.selectors
            .iter()
            .any(|selector| selector.contains(index))
    }
}

impl IndexSelector {
    #[must_use]
    pub const fn index(index: u32) -> Self {
        Self::Index(index)
    }

    #[must_use]
    pub const fn range(start: u32, end: u32) -> Self {
        Self::Range { start, end }
    }

    #[must_use]
    pub const fn contains(self, index: u32) -> bool {
        match self {
            Self::Index(candidate) => candidate == index,
            Self::Range { start, end } => start <= index && index <= end,
        }
    }
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
                    return parse_index_selection(page).map(selection_from_index_selection);
                }
                if looks_like_index_selection(&lower) {
                    return parse_index_selection(&lower).map(selection_from_index_selection);
                }
                Err(Error::InvalidInput(format!("invalid selection `{raw}`")))
            }
        }
    }
}

fn selection_from_index_selection(selection: IndexSelection) -> Selection {
    match selection.selectors.as_slice() {
        [IndexSelector::Index(index)] => Selection::Page(*index),
        _ => Selection::Indices(selection),
    }
}

fn parse_index_selection(text: &str) -> Result<IndexSelection> {
    let mut selectors = Vec::new();
    for token in text.split(',') {
        let token = token.trim();
        if token.is_empty() {
            return Err(Error::InvalidInput(format!(
                "invalid index selection `{text}`"
            )));
        }
        selectors.push(parse_index_selector(token)?);
    }
    IndexSelection::new(selectors)
}

fn parse_index_selector(text: &str) -> Result<IndexSelector> {
    if let Some((start, end)) = text.split_once('-') {
        let start = parse_u32(start.trim(), "index")?;
        let end = parse_u32(end.trim(), "index")?;
        return Ok(IndexSelector::Range { start, end });
    }
    parse_u32(text, "index").map(IndexSelector::Index)
}

fn looks_like_index_selection(text: &str) -> bool {
    text.bytes()
        .all(|byte| byte.is_ascii_digit() || matches!(byte, b',' | b'-' | b' ' | b'\t'))
}

fn validate_index_selector(selector: IndexSelector) -> Result<()> {
    match selector {
        IndexSelector::Index(0) | IndexSelector::Range { start: 0, .. } => Err(
            Error::InvalidInput("index selections are 1-based".to_owned()),
        ),
        IndexSelector::Range { start, end } if end == 0 || start > end => Err(Error::InvalidInput(
            format!("invalid index range `{start}-{end}`"),
        )),
        IndexSelector::Index(_) | IndexSelector::Range { .. } => Ok(()),
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

#[cfg(test)]
mod tests {
    use super::{IndexSelection, IndexSelector, Selection};
    use crate::Error;

    #[test]
    fn parses_single_bare_index_as_page_selection() -> anyhow::Result<()> {
        assert_eq!("2".parse::<Selection>()?, Selection::Page(2));
        Ok(())
    }

    #[test]
    fn parses_page_range_and_list_selection() -> anyhow::Result<()> {
        assert_eq!(
            "page:1,3-5,8".parse::<Selection>()?,
            Selection::Indices(IndexSelection::new([
                IndexSelector::Index(1),
                IndexSelector::Range { start: 3, end: 5 },
                IndexSelector::Index(8),
            ])?)
        );
        Ok(())
    }

    #[test]
    fn rejects_zero_and_reversed_ranges() {
        for input in ["0", "page:0", "page:3-1", "1,,2"] {
            let error = input.parse::<Selection>().err();
            assert!(
                matches!(error, Some(Error::InvalidInput(_))),
                "expected invalid input for {input:?}, got {error:?}"
            );
        }
    }
}
