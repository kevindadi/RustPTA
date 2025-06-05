use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::Hash;

#[derive(Debug, Eq, PartialEq)]
pub enum TopoligicalSortError {
    CycleDetected,
}

type TopologicalSortResult<Node> = Result<Vec<Node>, TopoligicalSortError>;





pub fn topological_sort<Node: Hash + Eq + Copy>(
    edges: &Vec<(Node, Node)>,
) -> TopologicalSortResult<Node> {
    
    
    
    let mut edges_by_source: HashMap<Node, Vec<Node>> = HashMap::default();
    let mut incoming_edges_count: HashMap<Node, usize> = HashMap::default();
    for (source, destination) in edges {
        incoming_edges_count.entry(*source).or_insert(0); 
        edges_by_source 
            .entry(*source)
            .or_default()
            .push(*destination);
        
        *incoming_edges_count.entry(*destination).or_insert(0) += 1;
    }

    
    
    let mut no_incoming_edges_q = VecDeque::default();
    for (node, count) in &incoming_edges_count {
        if *count == 0 {
            no_incoming_edges_q.push_back(*node);
        }
    }
    
    let mut sorted = Vec::default();
    while let Some(no_incoming_edges) = no_incoming_edges_q.pop_back() {
        sorted.push(no_incoming_edges); 
        incoming_edges_count.remove(&no_incoming_edges);
        
        for neighbour in edges_by_source.get(&no_incoming_edges).unwrap_or(&vec![]) {
            if let Some(count) = incoming_edges_count.get_mut(neighbour) {
                *count -= 1; 
                if *count == 0 {
                    
                    incoming_edges_count.remove(neighbour); 
                    no_incoming_edges_q.push_front(*neighbour); 
                }
            }
        }
    }
    if incoming_edges_count.is_empty() {
        
        Ok(sorted)
    } else {
        
        Err(TopoligicalSortError::CycleDetected)
    }
}

#[cfg(test)]
mod tests {
    use super::topological_sort;
    use crate::graph::topological_sort::TopoligicalSortError;

    fn is_valid_sort<Node: Eq>(sorted: &[Node], graph: &[(Node, Node)]) -> bool {
        for (source, dest) in graph {
            let source_pos = sorted.iter().position(|node| node == source);
            let dest_pos = sorted.iter().position(|node| node == dest);
            match (source_pos, dest_pos) {
                (Some(src), Some(dst)) if src < dst => {}
                _ => {
                    return false;
                }
            };
        }
        true
    }

    #[test]
    fn it_works() {
        let graph = vec![(1, 2), (1, 3), (2, 3), (3, 4), (4, 5), (5, 6), (6, 7)];
        let sort = topological_sort(&graph);
        assert!(sort.is_ok());
        let sort = sort.unwrap();
        assert!(is_valid_sort(&sort, &graph));
        assert_eq!(sort, vec![1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn test_wikipedia_example() {
        let graph = vec![
            (5, 11),
            (7, 11),
            (7, 8),
            (3, 8),
            (3, 10),
            (11, 2),
            (11, 9),
            (11, 10),
            (8, 9),
        ];
        let sort = topological_sort(&graph);
        assert!(sort.is_ok());
        let sort = sort.unwrap();
        assert!(is_valid_sort(&sort, &graph));
    }

    #[test]
    fn test_cyclic_graph() {
        let graph = vec![(1, 2), (2, 3), (3, 4), (4, 5), (4, 2)];
        let sort = topological_sort(&graph);
        assert!(sort.is_err());
        assert_eq!(sort.err().unwrap(), TopoligicalSortError::CycleDetected);
    }
}
