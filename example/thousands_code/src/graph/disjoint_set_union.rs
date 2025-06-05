





pub struct DSUNode {
    
    parent: usize,
    
    size: usize,
}




pub struct DisjointSetUnion {
    
    nodes: Vec<DSUNode>,
}

impl DisjointSetUnion {
    
    
    
    
    
    
    
    
    
    pub fn new(num_elements: usize) -> DisjointSetUnion {
        let mut nodes = Vec::with_capacity(num_elements + 1);
        for idx in 0..=num_elements {
            nodes.push(DSUNode {
                parent: idx,
                size: 1,
            });
        }

        Self { nodes }
    }

    
    
    
    
    
    
    
    
    
    
    
    
    pub fn find_set(&mut self, element: usize) -> usize {
        if element != self.nodes[element].parent {
            self.nodes[element].parent = self.find_set(self.nodes[element].parent);
        }
        self.nodes[element].parent
    }

    
    
    
    
    
    
    
    
    
    
    
    
    pub fn merge(&mut self, first_elem: usize, sec_elem: usize) -> usize {
        let mut first_root = self.find_set(first_elem);
        let mut sec_root = self.find_set(sec_elem);

        if first_root == sec_root {
            
            return usize::MAX;
        }

        
        if self.nodes[first_root].size < self.nodes[sec_root].size {
            std::mem::swap(&mut first_root, &mut sec_root);
        }

        self.nodes[sec_root].parent = first_root;
        self.nodes[first_root].size += self.nodes[sec_root].size;

        first_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disjoint_set_union() {
        let mut dsu = DisjointSetUnion::new(10);

        dsu.merge(1, 2);
        dsu.merge(2, 3);
        dsu.merge(1, 9);
        dsu.merge(4, 5);
        dsu.merge(7, 8);
        dsu.merge(4, 8);
        dsu.merge(6, 9);

        assert_eq!(dsu.find_set(1), dsu.find_set(2));
        assert_eq!(dsu.find_set(1), dsu.find_set(3));
        assert_eq!(dsu.find_set(1), dsu.find_set(6));
        assert_eq!(dsu.find_set(1), dsu.find_set(9));

        assert_eq!(dsu.find_set(4), dsu.find_set(5));
        assert_eq!(dsu.find_set(4), dsu.find_set(7));
        assert_eq!(dsu.find_set(4), dsu.find_set(8));

        assert_ne!(dsu.find_set(1), dsu.find_set(10));
        assert_ne!(dsu.find_set(4), dsu.find_set(10));

        dsu.merge(3, 4);

        assert_eq!(dsu.find_set(1), dsu.find_set(2));
        assert_eq!(dsu.find_set(1), dsu.find_set(3));
        assert_eq!(dsu.find_set(1), dsu.find_set(6));
        assert_eq!(dsu.find_set(1), dsu.find_set(9));
        assert_eq!(dsu.find_set(1), dsu.find_set(4));
        assert_eq!(dsu.find_set(1), dsu.find_set(5));
        assert_eq!(dsu.find_set(1), dsu.find_set(7));
        assert_eq!(dsu.find_set(1), dsu.find_set(8));

        assert_ne!(dsu.find_set(1), dsu.find_set(10));

        dsu.merge(10, 1);
        assert_eq!(dsu.find_set(10), dsu.find_set(1));
        assert_eq!(dsu.find_set(10), dsu.find_set(2));
        assert_eq!(dsu.find_set(10), dsu.find_set(3));
        assert_eq!(dsu.find_set(10), dsu.find_set(4));
        assert_eq!(dsu.find_set(10), dsu.find_set(5));
        assert_eq!(dsu.find_set(10), dsu.find_set(6));
        assert_eq!(dsu.find_set(10), dsu.find_set(7));
        assert_eq!(dsu.find_set(10), dsu.find_set(8));
        assert_eq!(dsu.find_set(10), dsu.find_set(9));
    }
}
