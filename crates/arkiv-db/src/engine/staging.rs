use std::collections::{BTreeMap, HashSet};

use arkiv_db_storage::versioned::{ObjectId, SpaceId};
use arkiv_db_trie::ObjectSink;

use crate::{
    codec::B256,
    layout::{NamespaceState, Posting},
    state::{MapKind, StagedObject},
};

use super::WorkingState;

impl WorkingState {
    /// Test aid: the staging walk over the whole state, see
    /// [`Snapshot::staged_objects`] and [`SealedCandidate::hashed_nodes`].
    pub(super) fn staging(&self) -> Staging {
        let mut staging = Staging::default();
        let mut leaves = Vec::new();
        self.catalog.stage_with(
            &mut staging.sink(SpaceId::Catalog, MapKind::Catalog),
            &mut |leaf: &NamespaceState| leaves.push(leaf.clone()),
        );
        for leaf in leaves {
            let space = SpaceId::Namespace(leaf.descriptor.id.ordinal());
            leaf.records
                .stage(&mut staging.sink(space, MapKind::Records));
            leaf.typed
                .keys
                .stage(&mut staging.sink(space, MapKind::Keys));
            leaf.typed
                .rows
                .stage(&mut staging.sink(space, MapKind::Rows));
            let mut postings = Vec::new();
            leaf.typed.terms.stage_with(
                &mut staging.sink(space, MapKind::Terms),
                &mut |posting: &Posting| postings.push(posting.clone()),
            );
            for posting in postings {
                posting.0.stage(&mut staging.sink(space, MapKind::Postings));
            }
        }
        self.host
            .stage(&mut staging.sink(SpaceId::Host, MapKind::Host));
        staging
    }
}

/// Staged objects of several maps, each `(space, map, id)` kept once. The
/// same bytes staged by two maps of one space appear twice; objects are put
/// if absent, so a commit writes them once.
///
/// `recorded` counts, per map, every node the walks recorded before
/// deduplication: identical nodes built separately are each counted.
#[derive(Default)]
pub(super) struct Staging {
    pub(super) objects: Vec<StagedObject>,
    seen: HashSet<(SpaceId, MapKind, ObjectId)>,
    pub(super) recorded: BTreeMap<MapKind, u64>,
}

impl Staging {
    fn sink(&mut self, space: SpaceId, map: MapKind) -> MapSink<'_> {
        MapSink {
            space,
            map,
            staging: self,
        }
    }
}

/// The sink of one map in one object space.
struct MapSink<'s> {
    space: SpaceId,
    map: MapKind,
    staging: &'s mut Staging,
}

impl ObjectSink for MapSink<'_> {
    fn record(&mut self, id: B256, encoding: &[u8]) {
        *self.staging.recorded.entry(self.map).or_default() += 1;
        if self.staging.seen.insert((self.space, self.map, id.0)) {
            self.staging.objects.push(StagedObject {
                space: self.space,
                map: self.map,
                id: id.0,
                bytes: encoding.to_vec(),
            });
        }
    }
}
