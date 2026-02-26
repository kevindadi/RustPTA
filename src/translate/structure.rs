use std::collections::HashMap;
use std::collections::hash_map::Entry;

use regex::Regex;
use rustc_hir::def_id::DefId;

use crate::concurrency::atomic::AtomicOrdering;
use crate::config::PnConfig;
use crate::{memory::pointsto::AliasId, net::PlaceId};

pub struct ResourceRegistry {
    locks: HashMap<AliasId, PlaceId>,
    condvars: HashMap<AliasId, PlaceId>,
    /// 每个 alias 可映射到多个 place（消除 first match 后，一个指针可能别名多个原子变量）
    atomic_places: HashMap<AliasId, Vec<PlaceId>>,
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

    pub fn atomic_places(&self) -> &HashMap<AliasId, Vec<PlaceId>> {
        &self.atomic_places
    }

    pub fn atomic_places_mut(&mut self) -> &mut HashMap<AliasId, Vec<PlaceId>> {
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
    pub fn new(config: &PnConfig) -> Self {
        let make_regex = |patterns: &[String]| -> Regex {
            if patterns.is_empty() {
                Regex::new("^$").unwrap() // Match nothing
            } else {
                let combined = patterns.join("|");
                Regex::new(&combined).expect(&format!("Invalid regex in config: {}", combined))
            }
        };

        Self {
            thread_spawn: make_regex(&config.thread_spawn),
            thread_join: make_regex(&config.thread_join),
            scope_spwan: make_regex(&config.scope_spawn),
            scope_join: make_regex(&config.scope_join),
            condvar_notify: make_regex(&config.condvar_notify),
            condvar_wait: make_regex(&config.condvar_wait),
            channel_send: make_regex(&config.channel_send),
            channel_recv: make_regex(&config.channel_recv),
            atomic_load: make_regex(&config.atomic_load),
            atomic_store: make_regex(&config.atomic_store),
        }
    }
}
