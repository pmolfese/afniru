//! `DatasetStore`: datasets by id behind `Arc`, so a cloned controller shares
//! voxels with the original instead of copying them.

use std::sync::Arc;

use crate::data::Dataset;

/// Identifies a dataset in the [`DatasetStore`] for the life of the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DatasetId(pub usize);

/// All opened datasets, in the order they were opened.
#[derive(Debug, Default)]
pub struct DatasetStore {
    items: Vec<Arc<Dataset>>,
}

impl DatasetStore {
    /// Store a dataset and return its id.
    pub fn add(&mut self, dataset: Dataset) -> DatasetId {
        self.items.push(Arc::new(dataset));
        DatasetId(self.items.len() - 1)
    }

    /// The dataset with this id.
    pub fn get(&self, id: DatasetId) -> Option<&Arc<Dataset>> {
        self.items.get(id.0)
    }

    /// Every dataset with its id, in opening order.
    pub fn iter(&self) -> impl Iterator<Item = (DatasetId, &Arc<Dataset>)> {
        self.items
            .iter()
            .enumerate()
            .map(|(i, d)| (DatasetId(i), d))
    }

    /// Number of datasets.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True when nothing has been opened.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::synthetic;

    #[test]
    fn ids_are_stable_and_in_opening_order() {
        let mut s = DatasetStore::default();
        assert!(s.is_empty());
        let a = s.add(synthetic::phantom());
        let b = s.add(synthetic::phantom());
        assert_ne!(a, b);
        assert_eq!(s.len(), 2);
        assert!(s.get(DatasetId(2)).is_none());
        assert_eq!(s.iter().map(|(id, _)| id).collect::<Vec<_>>(), [a, b]);
    }
}
