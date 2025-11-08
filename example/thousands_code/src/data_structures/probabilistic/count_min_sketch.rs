use std::collections::hash_map::RandomState;
use std::fmt::{Debug, Formatter};
use std::hash::{BuildHasher, Hash};













pub trait CountMinSketch {
    type Item;

    fn increment(&mut self, item: Self::Item);
    fn increment_by(&mut self, item: Self::Item, count: usize);
    fn get_count(&self, item: Self::Item) -> usize;
}



































































pub struct HashCountMinSketch<Item: Hash, const WIDTH: usize, const DEPTH: usize> {
    phantom: std::marker::PhantomData<Item>, 
    counts: [[usize; WIDTH]; DEPTH],
    hashers: [RandomState; DEPTH],
}

impl<Item: Hash, const WIDTH: usize, const DEPTH: usize> Debug
    for HashCountMinSketch<Item, WIDTH, DEPTH>
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Item").field("vecs", &self.counts).finish()
    }
}

impl<T: Hash, const WIDTH: usize, const DEPTH: usize> Default
    for HashCountMinSketch<T, WIDTH, DEPTH>
{
    fn default() -> Self {
        let hashers = std::array::from_fn(|_| RandomState::new());

        Self {
            phantom: Default::default(),
            counts: [[0; WIDTH]; DEPTH],
            hashers,
        }
    }
}

impl<Item: Hash, const WIDTH: usize, const DEPTH: usize> CountMinSketch
    for HashCountMinSketch<Item, WIDTH, DEPTH>
{
    type Item = Item;

    fn increment(&mut self, item: Self::Item) {
        self.increment_by(item, 1)
    }

    fn increment_by(&mut self, item: Self::Item, count: usize) {
        for (row, r) in self.hashers.iter_mut().enumerate() {
            let mut h = r.build_hasher();
            item.hash(&mut h);
            let hashed = r.hash_one(&item);
            let col = (hashed % WIDTH as u64) as usize;
            self.counts[row][col] += count;
        }
    }

    fn get_count(&self, item: Self::Item) -> usize {
        self.hashers
            .iter()
            .enumerate()
            .map(|(row, r)| {
                let mut h = r.build_hasher();
                item.hash(&mut h);
                let hashed = r.hash_one(&item);
                let col = (hashed % WIDTH as u64) as usize;
                self.counts[row][col]
            })
            .min()
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use crate::data_structures::probabilistic::count_min_sketch::{
        CountMinSketch, HashCountMinSketch,
    };
    use quickcheck::{Arbitrary, Gen};
    use std::collections::HashSet;

    #[test]
    fn hash_functions_should_hash_differently() {
        let mut sketch: HashCountMinSketch<&str, 50, 50> = HashCountMinSketch::default(); 
        sketch.increment("something");
        
        let mut indices_of_ones: HashSet<usize> = HashSet::default();
        for counts in sketch.counts {
            let ones = counts
                .into_iter()
                .enumerate()
                .filter_map(|(idx, count)| (count == 1).then_some(idx))
                .collect::<Vec<_>>();
            assert_eq!(1, ones.len());
            indices_of_ones.insert(ones[0]);
        }
        
        assert!(indices_of_ones.len() > 1); 
    }

    #[test]
    fn inspect_counts() {
        let mut sketch: HashCountMinSketch<&str, 5, 7> = HashCountMinSketch::default();
        sketch.increment("test");
        
        for counts in sketch.counts {
            let zeroes = counts.iter().filter(|count| **count == 0).count();
            assert_eq!(4, zeroes);
            let ones = counts.iter().filter(|count| **count == 1).count();
            assert_eq!(1, ones);
        }
        sketch.increment("test");
        for counts in sketch.counts {
            let zeroes = counts.iter().filter(|count| **count == 0).count();
            assert_eq!(4, zeroes);
            let twos = counts.iter().filter(|count| **count == 2).count();
            assert_eq!(1, twos);
        }

        
        assert_eq!(2, sketch.get_count("test"));
    }

    #[derive(Debug, Clone, Eq, PartialEq, Hash)]
    struct TestItem {
        item: String,
        count: usize,
    }

    const MAX_STR_LEN: u8 = 30;
    const MAX_COUNT: usize = 20;

    impl Arbitrary for TestItem {
        fn arbitrary(g: &mut Gen) -> Self {
            let str_len = u8::arbitrary(g) % MAX_STR_LEN;
            let mut str = String::with_capacity(str_len as usize);
            for _ in 0..str_len {
                str.push(char::arbitrary(g));
            }
            let count = usize::arbitrary(g) % MAX_COUNT;
            TestItem { item: str, count }
        }
    }

    #[quickcheck_macros::quickcheck]
    fn must_not_understimate_count(test_items: Vec<TestItem>) {
        let test_items = test_items.into_iter().collect::<HashSet<_>>(); 
        let n = test_items.len();
        let mut sketch: HashCountMinSketch<String, 50, 10> = HashCountMinSketch::default();
        let mut exact_count = 0;
        for TestItem { item, count } in &test_items {
            sketch.increment_by(item.clone(), *count);
        }
        for TestItem { item, count } in test_items {
            let stored_count = sketch.get_count(item);
            assert!(stored_count >= count);
            if count == stored_count {
                exact_count += 1;
            }
        }
        if n > 20 {
            
            let exact_ratio = exact_count as f64 / n as f64;
            assert!(exact_ratio > 0.7); 
        }
    }
}
