//! `Store<Id, T>`: a typed, ordered table of records keyed by stable ID.
//!
//! Stores are `BTreeMap`s so iteration order is the ID order, which keeps the sim
//! deterministic. In JSON a store is an object keyed by ID.

use std::borrow::{Borrow, Cow};
use std::collections::BTreeMap;
use std::collections::btree_map;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

/// A record that knows its own ID. Stores check the key matches.
pub trait Record {
    type Id: Ord + Clone;
    fn id(&self) -> &Self::Id;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
#[serde(bound(
    serialize = "K: Serialize, V: Serialize",
    deserialize = "K: Ord + Deserialize<'de>, V: Deserialize<'de>"
))]
pub struct Store<K: Ord, V> {
    items: BTreeMap<K, V>,
}

impl<K: Ord, V> Default for Store<K, V> {
    fn default() -> Self {
        Self {
            items: BTreeMap::new(),
        }
    }
}

impl<K: Ord, V> Store<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get<Q>(&self, id: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.items.get(id)
    }

    pub fn get_mut<Q>(&mut self, id: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.items.get_mut(id)
    }

    pub fn contains<Q>(&self, id: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.items.contains_key(id)
    }

    /// Inserts or replaces, returning the previous record.
    pub fn insert(&mut self, id: K, value: V) -> Option<V> {
        self.items.insert(id, value)
    }

    pub fn remove<Q>(&mut self, id: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.items.remove(id)
    }

    /// Records in ID order.
    pub fn iter(&self) -> btree_map::Iter<'_, K, V> {
        self.items.iter()
    }

    pub fn iter_mut(&mut self) -> btree_map::IterMut<'_, K, V> {
        self.items.iter_mut()
    }

    pub fn keys(&self) -> btree_map::Keys<'_, K, V> {
        self.items.keys()
    }

    pub fn values(&self) -> btree_map::Values<'_, K, V> {
        self.items.values()
    }
}

impl<V: Record> Store<V::Id, V> {
    /// Inserts a record under its own ID, returning the previous one.
    pub fn put(&mut self, value: V) -> Option<V> {
        self.items.insert(value.id().clone(), value)
    }
}

impl<V: Record> FromIterator<V> for Store<V::Id, V> {
    fn from_iter<I: IntoIterator<Item = V>>(iter: I) -> Self {
        let mut s = Store::new();
        for v in iter {
            s.put(v);
        }
        s
    }
}

impl<'a, K: Ord, V> IntoIterator for &'a Store<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = btree_map::Iter<'a, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<K: Ord + JsonSchema, V: JsonSchema> JsonSchema for Store<K, V> {
    fn inline_schema() -> bool {
        true
    }
    fn schema_name() -> Cow<'static, str> {
        <BTreeMap<K, V>>::schema_name()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        <BTreeMap<K, V>>::json_schema(generator)
    }
}
