use std::{
    cmp::Ordering,
    iter::FromIterator,
    mem,
    ops::Not,
    time::{SystemTime, UNIX_EPOCH},
};


struct TreapNode<T: Ord> {
    value: T,
    priority: usize,
    left: Option<Box<TreapNode<T>>>,
    right: Option<Box<TreapNode<T>>>,
}






pub struct Treap<T: Ord> {
    root: Option<Box<TreapNode<T>>>,
    length: usize,
}


#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

impl<T: Ord> Treap<T> {
    pub fn new() -> Treap<T> {
        Treap {
            root: None,
            length: 0,
        }
    }

    
    pub fn contains(&self, value: &T) -> bool {
        let mut current = &self.root;
        while let Some(node) = current {
            current = match value.cmp(&node.value) {
                Ordering::Equal => return true,
                Ordering::Less => &node.left,
                Ordering::Greater => &node.right,
            }
        }
        false
    }

    
    
    
    pub fn insert(&mut self, value: T) -> bool {
        let inserted = insert(&mut self.root, value);
        if inserted {
            self.length += 1;
        }
        inserted
    }

    
    
    
    pub fn remove(&mut self, value: &T) -> bool {
        let removed = remove(&mut self.root, value);
        if removed {
            self.length -= 1;
        }
        removed
    }

    
    pub fn len(&self) -> usize {
        self.length
    }

    
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    
    fn node_iter(&self) -> NodeIter<T> {
        let mut node_iter = NodeIter { stack: Vec::new() };
        
        let mut child = &self.root;
        while let Some(node) = child {
            node_iter.stack.push(node.as_ref());
            child = &node.left;
        }
        node_iter
    }

    
    pub fn iter(&self) -> Iter<T> {
        Iter {
            node_iter: self.node_iter(),
        }
    }
}


fn rand() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos() as usize
}


fn insert<T: Ord>(tree: &mut Option<Box<TreapNode<T>>>, value: T) -> bool {
    if let Some(node) = tree {
        let inserted = match value.cmp(&node.value) {
            Ordering::Equal => false,
            Ordering::Less => insert(&mut node.left, value),
            Ordering::Greater => insert(&mut node.right, value),
        };
        if inserted {
            node.rebalance();
        }
        inserted
    } else {
        *tree = Some(Box::new(TreapNode {
            value,
            priority: rand(),
            left: None,
            right: None,
        }));
        true
    }
}


fn remove<T: Ord>(tree: &mut Option<Box<TreapNode<T>>>, value: &T) -> bool {
    if let Some(node) = tree {
        let removed = match value.cmp(&node.value) {
            Ordering::Less => remove(&mut node.left, value),
            Ordering::Greater => remove(&mut node.right, value),
            Ordering::Equal => {
                *tree = match (node.left.take(), node.right.take()) {
                    (None, None) => None,
                    (Some(b), None) | (None, Some(b)) => Some(b),
                    (Some(left), Some(right)) => {
                        let side = match left.priority.cmp(&right.priority) {
                            Ordering::Greater => Side::Right,
                            _ => Side::Left,
                        };
                        node.left = Some(left);
                        node.right = Some(right);
                        node.rotate(side);
                        remove(node.child_mut(side), value);
                        Some(tree.take().unwrap())
                    }
                };
                return true;
            }
        };
        if removed {
            node.rebalance();
        }
        removed
    } else {
        false
    }
}

impl<T: Ord> TreapNode<T> {
    
    fn child(&self, side: Side) -> &Option<Box<TreapNode<T>>> {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    
    fn child_mut(&mut self, side: Side) -> &mut Option<Box<TreapNode<T>>> {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    
    fn priority(&self, side: Side) -> usize {
        self.child(side).as_ref().map_or(0, |n| n.priority)
    }

    
    fn rotate(&mut self, side: Side) {
        if self.child_mut(!side).is_none() {
            return;
        }

        let mut subtree = self.child_mut(!side).take().unwrap();
        *self.child_mut(!side) = subtree.child_mut(side).take();
        
        mem::swap(self, subtree.as_mut());
        
        *self.child_mut(side) = Some(subtree);
    }

    
    fn rebalance(&mut self) {
        match (
            self.priority,
            self.priority(Side::Left),
            self.priority(Side::Right),
        ) {
            (v, p, q) if p >= q && p > v => self.rotate(Side::Right),
            (v, p, q) if p < q && q > v => self.rotate(Side::Left),
            _ => (),
        };
    }

    #[cfg(test)]
    fn is_valid(&self) -> bool {
        self.priority >= self.priority(Side::Left) && self.priority >= self.priority(Side::Right)
    }
}

impl<T: Ord> Default for Treap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl Not for Side {
    type Output = Side;

    fn not(self) -> Self::Output {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

impl<T: Ord> FromIterator<T> for Treap<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut tree = Treap::new();
        for value in iter {
            tree.insert(value);
        }
        tree
    }
}




struct NodeIter<'a, T: Ord> {
    stack: Vec<&'a TreapNode<T>>,
}

impl<'a, T: Ord> Iterator for NodeIter<'a, T> {
    type Item = &'a TreapNode<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(node) = self.stack.pop() {
            
            let mut child = &node.right;
            while let Some(subtree) = child {
                self.stack.push(subtree.as_ref());
                child = &subtree.left;
            }
            Some(node)
        } else {
            None
        }
    }
}




pub struct Iter<'a, T: Ord> {
    node_iter: NodeIter<'a, T>,
}

impl<'a, T: Ord> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<&'a T> {
        match self.node_iter.next() {
            Some(node) => Some(&node.value),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Treap;

    
    fn is_valid<T: Ord>(tree: &Treap<T>) -> bool {
        tree.node_iter().all(|n| n.is_valid())
    }

    #[test]
    fn len() {
        let tree: Treap<_> = (1..4).collect();
        assert_eq!(tree.len(), 3);
    }

    #[test]
    fn contains() {
        let tree: Treap<_> = (1..4).collect();
        assert!(tree.contains(&1));
        assert!(!tree.contains(&4));
    }

    #[test]
    fn insert() {
        let mut tree = Treap::new();
        
        assert!(tree.insert(1));
        
        assert!(!tree.insert(1));
    }

    #[test]
    fn remove() {
        let mut tree: Treap<_> = (1..8).collect();
        
        assert!(tree.remove(&4));
        
        assert!(!tree.remove(&4));
    }

    #[test]
    fn sorted() {
        let tree: Treap<_> = (1..8).rev().collect();
        assert!((1..8).eq(tree.iter().copied()));
    }

    #[test]
    fn valid() {
        let mut tree: Treap<_> = (1..8).collect();
        assert!(is_valid(&tree));
        for x in 1..8 {
            tree.remove(&x);
            assert!(is_valid(&tree));
        }
    }
}
