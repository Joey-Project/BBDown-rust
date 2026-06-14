use crate::{Error, IndexSelection, IndexSelector, Result, Selection};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FeedListFetchMode {
    All,
    Latest,
    Page(u32),
}

impl FeedListFetchMode {
    pub(crate) fn is_satisfied_by(self, item_count: usize) -> bool {
        match self {
            Self::All => false,
            Self::Latest => item_count > 0,
            Self::Page(page) => {
                page != 0 && usize::try_from(page).is_ok_and(|target| item_count >= target)
            }
        }
    }
}

pub(crate) fn feed_list_fetch_mode(selection: Option<&Selection>) -> Result<FeedListFetchMode> {
    match selection {
        Some(Selection::Current) => Err(Error::InvalidInput(
            "current selection is only valid for inputs that identify a single current item"
                .to_owned(),
        )),
        Some(Selection::Episode(_)) => Err(Error::InvalidInput(
            "episode selection is only valid for PGC inputs".to_owned(),
        )),
        Some(Selection::Latest) => Ok(FeedListFetchMode::Latest),
        Some(Selection::Page(page)) => Ok(FeedListFetchMode::Page(*page)),
        Some(Selection::Indices(indices)) => Ok(FeedListFetchMode::Page(indices.max_index())),
        Some(Selection::All) | None => Ok(FeedListFetchMode::All),
    }
}

pub(crate) fn feed_list_info_fetch_mode(
    selection: Option<&Selection>,
) -> Result<FeedListFetchMode> {
    feed_list_fetch_mode(selection).map(|_| FeedListFetchMode::All)
}

pub(crate) fn select_feed_list_items<T: Clone>(
    items: &[T],
    selection: Option<&Selection>,
    index_of: impl Fn(&T) -> u32,
    missing_field: &'static str,
) -> Result<Vec<T>> {
    let selected_items = match selection {
        Some(Selection::Latest) => items.first().cloned().into_iter().collect(),
        Some(Selection::Page(page)) => items
            .iter()
            .find(|item| index_of(item) == *page)
            .cloned()
            .into_iter()
            .collect(),
        Some(Selection::Indices(indices)) => {
            select_items_by_index(items, indices, index_of, missing_field)?
        }
        Some(Selection::Episode(_)) => {
            return Err(Error::InvalidInput(
                "episode selection is only valid for PGC inputs".to_owned(),
            ));
        }
        Some(Selection::Current) => {
            return Err(Error::InvalidInput(
                "current selection is only valid for inputs that identify a single current item"
                    .to_owned(),
            ));
        }
        Some(Selection::All) | None => items.to_vec(),
    };
    let allow_empty_selection = matches!(selection, Some(Selection::All) | None);
    if selected_items.is_empty() && !allow_empty_selection {
        return Err(Error::MissingField(missing_field));
    }
    Ok(selected_items)
}

pub(crate) fn select_items_by_index<T: Clone>(
    items: &[T],
    selection: &IndexSelection,
    index_of: impl Fn(&T) -> u32,
    missing_field: &'static str,
) -> Result<Vec<T>> {
    let indexed_items = items
        .iter()
        .map(|item| (index_of(item), item))
        .collect::<Vec<_>>();
    let available_indices = indexed_items
        .iter()
        .map(|(index, _)| *index)
        .collect::<HashSet<_>>();
    for selector in selection.selectors() {
        validate_index_selector_matches(*selector, &available_indices, missing_field)?;
    }

    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for selector in selection.selectors() {
        for (index, item) in &indexed_items {
            if selector.contains(*index) && seen.insert(*index) {
                selected.push((*item).clone());
            }
        }
    }
    Ok(selected)
}

pub(crate) fn push_unique_feed_list_item<T>(
    items: &mut Vec<T>,
    item: T,
    same_identity: impl Fn(&T, &T) -> bool,
) {
    if items.iter().any(|existing| same_identity(existing, &item)) {
        return;
    }
    items.push(item);
}

pub(crate) fn renumber_feed_list_items<T>(items: &mut [T], mut set_index: impl FnMut(&mut T, u32)) {
    for (index, item) in items.iter_mut().enumerate() {
        if let Ok(next_index) = u32::try_from(index + 1) {
            set_index(item, next_index);
        }
    }
}

fn validate_index_selector_matches(
    selector: IndexSelector,
    available_indices: &HashSet<u32>,
    missing_field: &'static str,
) -> Result<()> {
    match selector {
        IndexSelector::Index(index) => {
            if available_indices.contains(&index) {
                Ok(())
            } else {
                Err(Error::MissingField(missing_field))
            }
        }
        IndexSelector::Range { start, end } => {
            let requested_count = u64::from(end) - u64::from(start) + 1;
            if requested_count > available_indices.len() as u64 {
                return Err(Error::MissingField(missing_field));
            }
            if (start..=end).all(|index| available_indices.contains(&index)) {
                Ok(())
            } else {
                Err(Error::MissingField(missing_field))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FeedListFetchMode, feed_list_fetch_mode, push_unique_feed_list_item,
        renumber_feed_list_items, select_feed_list_items,
    };
    use crate::{Error, IndexSelection, IndexSelector, Selection};

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct TestItem {
        index: u32,
        identity: u32,
        title: &'static str,
    }

    #[test]
    fn feed_list_index_fetch_mode_stops_after_max_requested_index() -> anyhow::Result<()> {
        let selection = IndexSelection::new([IndexSelector::index(1), IndexSelector::range(3, 4)])?;
        let fetch_mode = feed_list_fetch_mode(Some(&Selection::Indices(selection)))?;

        assert_eq!(fetch_mode, FeedListFetchMode::Page(4));
        assert!(!fetch_mode.is_satisfied_by(3));
        assert!(fetch_mode.is_satisfied_by(4));
        Ok(())
    }

    #[test]
    fn feed_list_selection_preserves_selector_order_and_deduplicates() -> anyhow::Result<()> {
        let items = [
            test_item(1, "Newest video"),
            test_item(2, "Middle video"),
            test_item(3, "Older video"),
        ];
        let selection = IndexSelection::new([
            IndexSelector::range(2, 3),
            IndexSelector::index(1),
            IndexSelector::index(2),
        ])?;

        let selected = select_feed_list_items(
            &items,
            Some(&Selection::Indices(selection)),
            |item| item.index,
            "selected feed item",
        )?;

        assert_eq!(
            selected.iter().map(|item| item.title).collect::<Vec<_>>(),
            ["Middle video", "Older video", "Newest video"]
        );
        Ok(())
    }

    #[test]
    fn feed_list_all_selection_allows_empty_items() -> anyhow::Result<()> {
        let default_selected =
            select_feed_list_items::<TestItem>(&[], None, |item| item.index, "selected feed item")?;
        let all_selected = select_feed_list_items::<TestItem>(
            &[],
            Some(&Selection::All),
            |item| item.index,
            "selected feed item",
        )?;

        assert!(default_selected.is_empty());
        assert!(all_selected.is_empty());
        Ok(())
    }

    #[test]
    fn feed_list_current_selection_is_rejected() {
        let items = [test_item(1, "Saved video")];
        let error = select_feed_list_items(
            &items,
            Some(&Selection::Current),
            |item| item.index,
            "selected feed item",
        )
        .err();

        assert!(
            matches!(error, Some(Error::InvalidInput(message)) if message.contains("current selection"))
        );
    }

    #[test]
    fn feed_list_items_can_be_deduplicated_and_renumbered() {
        let mut items = Vec::new();
        push_unique_feed_list_item(&mut items, test_identity_item(0, 10), |left, right| {
            left.identity == right.identity
        });
        push_unique_feed_list_item(&mut items, test_identity_item(0, 10), |left, right| {
            left.identity == right.identity
        });
        push_unique_feed_list_item(&mut items, test_identity_item(0, 20), |left, right| {
            left.identity == right.identity
        });

        renumber_feed_list_items(&mut items, |item, index| item.index = index);

        assert_eq!(
            items
                .iter()
                .map(|item| (item.index, item.identity))
                .collect::<Vec<_>>(),
            [(1, 10), (2, 20)]
        );
    }

    fn test_item(index: u32, title: &'static str) -> TestItem {
        TestItem {
            index,
            identity: index,
            title,
        }
    }

    fn test_identity_item(index: u32, identity: u32) -> TestItem {
        TestItem {
            index,
            identity,
            title: "item",
        }
    }
}
