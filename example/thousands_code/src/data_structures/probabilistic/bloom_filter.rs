use std::collections::hash_map::{DefaultHasher, RandomState};
use std::hash::{BuildHasher, Hash, Hasher};



pub trait BloomFilter<Item: Hash> {
    fn insert(&mut self, item: Item);
    fn contains(&self, item: &Item) -> bool;
}














#[derive(Debug)]
struct BasicBloomFilter<const CAPACITY: usize> {
    vec: [bool; CAPACITY],
}

impl<const CAPACITY: usize> Default for BasicBloomFilter<CAPACITY> {
    fn default() -> Self {
        Self {
            vec: [false; CAPACITY],
        }
    }
}

impl<Item: Hash, const CAPACITY: usize> BloomFilter<Item> for BasicBloomFilter<CAPACITY> {
    fn insert(&mut self, item: Item) {
        let mut hasher = DefaultHasher::new();
        item.hash(&mut hasher);
        let idx = (hasher.finish() % CAPACITY as u64) as usize;
        self.vec[idx] = true;
    }

    fn contains(&self, item: &Item) -> bool {
        let mut hasher = DefaultHasher::new();
        item.hash(&mut hasher);
        let idx = (hasher.finish() % CAPACITY as u64) as usize;
        self.vec[idx]
    }
}










#[allow(dead_code)]
#[derive(Debug, Default)]
struct SingleBinaryBloomFilter {
    fingerprint: u128, 
}


fn mask_128<T: Hash>(hasher: &mut DefaultHasher, item: T) -> u128 {
    item.hash(hasher);
    let idx = (hasher.finish() % 128) as u32;
    
    2_u128.pow(idx)
}

impl<T: Hash> BloomFilter<T> for SingleBinaryBloomFilter {
    fn insert(&mut self, item: T) {
        self.fingerprint |= mask_128(&mut DefaultHasher::new(), &item);
    }

    fn contains(&self, item: &T) -> bool {
        (self.fingerprint & mask_128(&mut DefaultHasher::new(), item)) > 0
    }
}


















pub struct MultiBinaryBloomFilter {
    filter_size: usize,
    bytes: Vec<u8>,
    hash_builders: Vec<RandomState>,
}

impl MultiBinaryBloomFilter {
    pub fn with_dimensions(filter_size: usize, hash_count: usize) -> Self {
        let bytes_count = filter_size / 8 + if filter_size % 8 > 0 { 1 } else { 0 }; 
        Self {
            filter_size,
            bytes: vec![0; bytes_count],
            hash_builders: vec![RandomState::new(); hash_count],
        }
    }

    pub fn from_estimate(
        estimated_count_of_items: usize,
        max_false_positive_probability: f64,
    ) -> Self {
        
        let optimal_filter_size = (-(estimated_count_of_items as f64)
            * max_false_positive_probability.ln()
            / (2.0_f64.ln().powi(2)))
        .ceil() as usize;
        let optimal_hash_count = ((optimal_filter_size as f64 / estimated_count_of_items as f64)
            * 2.0_f64.ln())
        .ceil() as usize;
        Self::with_dimensions(optimal_filter_size, optimal_hash_count)
    }
}

impl<Item: Hash> BloomFilter<Item> for MultiBinaryBloomFilter {
    fn insert(&mut self, item: Item) {
        for builder in &self.hash_builders {
            let mut hasher = builder.build_hasher();
            item.hash(&mut hasher);
            let hash = builder.hash_one(&item);
            let index = hash % self.filter_size as u64;
            let byte_index = index as usize / 8; 
            let bit_index = (index % 8) as u8; 
            self.bytes[byte_index] |= 1 << bit_index;
        }
    }

    fn contains(&self, item: &Item) -> bool {
        for builder in &self.hash_builders {
            let mut hasher = builder.build_hasher();
            item.hash(&mut hasher);
            let hash = builder.hash_one(item);
            let index = hash % self.filter_size as u64;
            let byte_index = index as usize / 8; 
            let bit_index = (index % 8) as u8; 
            if self.bytes[byte_index] & (1 << bit_index) == 0 {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::data_structures::probabilistic::bloom_filter::{
        BasicBloomFilter, BloomFilter, MultiBinaryBloomFilter, SingleBinaryBloomFilter,
    };
    use quickcheck::{Arbitrary, Gen};
    use quickcheck_macros::quickcheck;
    use std::collections::HashSet;

    #[derive(Debug, Clone)]
    struct TestSet {
        to_insert: HashSet<i32>,
        to_test: Vec<i32>,
    }

    impl Arbitrary for TestSet {
        fn arbitrary(g: &mut Gen) -> Self {
            let mut qty = usize::arbitrary(g) % 5_000;
            if qty < 50 {
                qty += 50; 
            }
            let mut to_insert = HashSet::with_capacity(qty);
            let mut to_test = Vec::with_capacity(qty);
            for _ in 0..(qty) {
                to_insert.insert(i32::arbitrary(g));
                to_test.push(i32::arbitrary(g));
            }
            TestSet { to_insert, to_test }
        }
    }

    #[quickcheck]
    fn basic_filter_must_not_return_false_negative(TestSet { to_insert, to_test }: TestSet) {
        let mut basic_filter = BasicBloomFilter::<10_000>::default();
        for item in &to_insert {
            basic_filter.insert(*item);
        }
        for other in to_test {
            if !basic_filter.contains(&other) {
                assert!(!to_insert.contains(&other))
            }
        }
    }

    #[quickcheck]
    fn binary_filter_must_not_return_false_negative(TestSet { to_insert, to_test }: TestSet) {
        let mut binary_filter = SingleBinaryBloomFilter::default();
        for item in &to_insert {
            binary_filter.insert(*item);
        }
        for other in to_test {
            if !binary_filter.contains(&other) {
                assert!(!to_insert.contains(&other))
            }
        }
    }

    #[quickcheck]
    fn a_basic_filter_of_capacity_128_is_the_same_as_a_binary_filter(
        TestSet { to_insert, to_test }: TestSet,
    ) {
        let mut basic_filter = BasicBloomFilter::<128>::default(); 
        let mut binary_filter = SingleBinaryBloomFilter::default();
        for item in &to_insert {
            basic_filter.insert(*item);
            binary_filter.insert(*item);
        }
        for other in to_test {
            
            assert_eq!(
                basic_filter.contains(&other),
                binary_filter.contains(&other)
            );
        }
    }

    const FALSE_POSITIVE_MAX: f64 = 0.05;

    #[quickcheck]
    fn a_multi_binary_bloom_filter_must_not_return_false_negatives(
        TestSet { to_insert, to_test }: TestSet,
    ) {
        let n = to_insert.len();
        if n == 0 {
            
            return;
        }
        
        let mut binary_filter = MultiBinaryBloomFilter::from_estimate(n, FALSE_POSITIVE_MAX);
        for item in &to_insert {
            binary_filter.insert(*item);
        }
        let tests = to_test.len();
        let mut false_positives = 0;
        for other in to_test {
            if !binary_filter.contains(&other) {
                assert!(!to_insert.contains(&other))
            } else if !to_insert.contains(&other) {
                
                false_positives += 1;
            }
        }
        let fp_rate = false_positives as f64 / tests as f64;
        assert!(fp_rate < 1.0); 
    }
}
