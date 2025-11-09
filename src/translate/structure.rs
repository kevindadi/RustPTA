use std::collections::HashMap;
use std::collections::hash_map::Entry;

use regex::Regex;
use rustc_hir::def_id::DefId;

use crate::{memory::pointsto::AliasId, net::PlaceId};
use crate::concurrency::atomic::AtomicOrdering;

pub struct ResourceRegistry {
    locks: HashMap<AliasId, PlaceId>,
    condvars: HashMap<AliasId, PlaceId>,
    atomic_places: HashMap<AliasId, PlaceId>,
    atomic_orders: HashMap<AliasId, AtomicOrdering>,
    unsafe_places: HashMap<AliasId, PlaceId>,
    channel_places: HashMap<AliasId, PlaceId>,
}

impl ResourceRegistry {
    pub fn new() -> Self {
        Self {
            locks: HashMap::default(),
            condvars: HashMap::default(),
            atomic_places: HashMap::default(),
            atomic_orders: HashMap::default(),
            unsafe_places: HashMap::default(),
            channel_places: HashMap::default(),
        }
    }

    pub fn locks(&self) -> &HashMap<AliasId, PlaceId> {
        &self.locks
    }

    pub fn locks_mut(&mut self) -> &mut HashMap<AliasId, PlaceId> {
        &mut self.locks
    }

    pub fn condvars(&self) -> &HashMap<AliasId, PlaceId> {
        &self.condvars
    }

    pub fn condvars_mut(&mut self) -> &mut HashMap<AliasId, PlaceId> {
        &mut self.condvars
    }

    pub fn atomic_places(&self) -> &HashMap<AliasId, PlaceId> {
        &self.atomic_places
    }

    pub fn atomic_places_mut(&mut self) -> &mut HashMap<AliasId, PlaceId> {
        &mut self.atomic_places
    }

    pub fn atomic_orders(&self) -> &HashMap<AliasId, AtomicOrdering> {
        &self.atomic_orders
    }

    pub fn atomic_orders_mut(&mut self) -> &mut HashMap<AliasId, AtomicOrdering> {
        &mut self.atomic_orders
    }

    pub fn unsafe_places(&self) -> &HashMap<AliasId, PlaceId> {
        &self.unsafe_places
    }

    pub fn unsafe_places_mut(&mut self) -> &mut HashMap<AliasId, PlaceId> {
        &mut self.unsafe_places
    }

    pub fn channel_places(&self) -> &HashMap<AliasId, PlaceId> {
        &self.channel_places
    }

    pub fn channel_places_mut(&mut self) -> &mut HashMap<AliasId, PlaceId> {
        &mut self.channel_places
    }
}

pub struct FunctionRegistry {
    counter: HashMap<DefId, (PlaceId, PlaceId)>,
}

impl FunctionRegistry {
    pub fn new() -> Self {
        Self {
            counter: HashMap::new(),
        }
    }

    pub fn contains(&self, def_id: &DefId) -> bool {
        self.counter.contains_key(def_id)
    }

    pub fn insert(&mut self, def_id: DefId, start: PlaceId, end: PlaceId) {
        self.counter.insert(def_id, (start, end));
    }

    pub fn counter(&self) -> &HashMap<DefId, (PlaceId, PlaceId)> {
        &self.counter
    }

    pub fn get_or_insert<F>(&mut self, def_id: DefId, create: F) -> (PlaceId, PlaceId)
    where
        F: FnOnce() -> (PlaceId, PlaceId),
    {
        match self.counter.entry(def_id) {
            Entry::Occupied(existing) => *existing.get(),
            Entry::Vacant(vacant) => {
                let place_pair = create();
                vacant.insert(place_pair);
                place_pair
            }
        }
    }
}

pub struct KeyApiRegex {
    pub thread_spawn: Regex,
    pub thread_join: Regex,
    pub scope_spwan: Regex,
    pub scope_join: Regex,
    pub condvar_notify: Regex,
    pub condvar_wait: Regex,

    pub channel_send: Regex,
    pub channel_recv: Regex,

    pub atomic_load: Regex,
    pub atomic_store: Regex,
}

impl KeyApiRegex {
    pub fn new() -> Self {
        Self {
            thread_spawn: Regex::new(r"std::thread[:a-zA-Z0-9_#\{\}]*::spawn").unwrap(),
            thread_join: Regex::new(r"std::thread[:a-zA-Z0-9_#\{\}]*::join").unwrap(),
            scope_spwan: Regex::new(r"std::thread::scoped[:a-zA-Z0-9_#\{\}]*::spawn").unwrap(),
            scope_join: Regex::new(r"std::thread::scoped[:a-zA-Z0-9_#\{\}]*::join").unwrap(),
            condvar_notify: Regex::new(r"condvar[:a-zA-Z0-9_#\{\}]*::notify").unwrap(),
            condvar_wait: Regex::new(r"condvar[:a-zA-Z0-9_#\{\}]*::wait").unwrap(),
            channel_send: Regex::new(r"mpsc[:a-zA-Z0-9_#\{\}]*::send").unwrap(),
            channel_recv: Regex::new(r"mpsc[:a-zA-Z0-9_#\{\}]*::recv").unwrap(),
            atomic_load: Regex::new(r"atomic[:a-zA-Z0-9]*::load").unwrap(),
            atomic_store: Regex::new(r"atomic[:a-zA-Z0-9]*::store").unwrap(),
        }
    }
}
