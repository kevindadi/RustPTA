



#[derive(Debug, PartialEq, Eq)]
pub enum FindHamiltonianCycleError {
    
    EmptyAdjacencyMatrix,
    
    ImproperAdjacencyMatrix,
    
    StartOutOfBound,
}


struct Graph {
    
    adjacency_matrix: Vec<Vec<bool>>,
}

impl Graph {
    
    
    
    
    
    
    
    
    
    
    
    fn new(adjacency_matrix: Vec<Vec<bool>>) -> Result<Self, FindHamiltonianCycleError> {
        
        if adjacency_matrix.is_empty() {
            return Err(FindHamiltonianCycleError::EmptyAdjacencyMatrix);
        }

        
        if adjacency_matrix
            .iter()
            .any(|row| row.len() != adjacency_matrix.len())
        {
            return Err(FindHamiltonianCycleError::ImproperAdjacencyMatrix);
        }

        Ok(Self { adjacency_matrix })
    }

    
    fn num_vertices(&self) -> usize {
        self.adjacency_matrix.len()
    }

    
    
    
    
    
    
    
    
    
    
    
    
    fn is_safe(&self, v: usize, visited: &[bool], path: &[Option<usize>], pos: usize) -> bool {
        
        if !self.adjacency_matrix[path[pos - 1].unwrap()][v] {
            return false;
        }

        
        !visited[v]
    }

    
    
    
    
    
    
    
    
    
    
    
    
    
    fn hamiltonian_cycle_util(
        &self,
        path: &mut [Option<usize>],
        visited: &mut [bool],
        pos: usize,
    ) -> bool {
        if pos == self.num_vertices() {
            
            return self.adjacency_matrix[path[pos - 1].unwrap()][path[0].unwrap()];
        }

        for v in 0..self.num_vertices() {
            if self.is_safe(v, visited, path, pos) {
                path[pos] = Some(v);
                visited[v] = true;
                if self.hamiltonian_cycle_util(path, visited, pos + 1) {
                    return true;
                }
                path[pos] = None;
                visited[v] = false;
            }
        }

        false
    }

    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    fn find_hamiltonian_cycle(
        &self,
        start_vertex: usize,
    ) -> Result<Option<Vec<usize>>, FindHamiltonianCycleError> {
        
        if start_vertex >= self.num_vertices() {
            return Err(FindHamiltonianCycleError::StartOutOfBound);
        }

        
        let mut path = vec![None; self.num_vertices()];
        
        path[0] = Some(start_vertex);

        
        let mut visited = vec![false; self.num_vertices()];
        visited[start_vertex] = true;

        if self.hamiltonian_cycle_util(&mut path, &mut visited, 1) {
            
            path.push(Some(start_vertex));
            Ok(Some(path.into_iter().map(Option::unwrap).collect()))
        } else {
            Ok(None)
        }
    }
}


pub fn find_hamiltonian_cycle(
    adjacency_matrix: Vec<Vec<bool>>,
    start_vertex: usize,
) -> Result<Option<Vec<usize>>, FindHamiltonianCycleError> {
    Graph::new(adjacency_matrix)?.find_hamiltonian_cycle(start_vertex)
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! hamiltonian_cycle_tests {
        ($($name:ident: $test_case:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (adjacency_matrix, start_vertex, expected) = $test_case;
                    let result = find_hamiltonian_cycle(adjacency_matrix, start_vertex);
                    assert_eq!(result, expected);
                }
            )*
        };
    }

    hamiltonian_cycle_tests! {
        test_complete_graph: (
            vec![
                vec![false, true, true, true],
                vec![true, false, true, true],
                vec![true, true, false, true],
                vec![true, true, true, false],
            ],
            0,
            Ok(Some(vec![0, 1, 2, 3, 0]))
        ),
        test_directed_graph_with_cycle: (
            vec![
                vec![false, true, false, false, false],
                vec![false, false, true, true, false],
                vec![true, false, false, true, true],
                vec![false, false, true, false, true],
                vec![true, true, false, false, false],
            ],
            2,
            Ok(Some(vec![2, 3, 4, 0, 1, 2]))
        ),
        test_undirected_graph_with_cycle: (
            vec![
                vec![false, true, false, false, true],
                vec![true, false, true, false, false],
                vec![false, true, false, true, false],
                vec![false, false, true, false, true],
                vec![true, false, false, true, false],
            ],
            2,
            Ok(Some(vec![2, 1, 0, 4, 3, 2]))
        ),
        test_directed_graph_no_cycle: (
            vec![
                vec![false, true, false, true, false],
                vec![false, false, true, true, false],
                vec![false, false, false, true, false],
                vec![false, false, false, false, true],
                vec![false, false, true, false, false],
            ],
            0,
            Ok(None::<Vec<usize>>)
        ),
        test_undirected_graph_no_cycle: (
            vec![
                vec![false, true, false, false, false],
                vec![true, false, true, true, false],
                vec![false, true, false, true, true],
                vec![false, true, true, false, true],
                vec![false, false, true, true, false],
            ],
            0,
            Ok(None::<Vec<usize>>)
        ),
        test_triangle_graph: (
            vec![
                vec![false, true, false],
                vec![false, false, true],
                vec![true, false, false],
            ],
            1,
            Ok(Some(vec![1, 2, 0, 1]))
        ),
        test_tree_graph: (
            vec![
                vec![false, true, false, true, false],
                vec![true, false, true, true, false],
                vec![false, true, false, false, false],
                vec![true, true, false, false, true],
                vec![false, false, false, true, false],
            ],
            0,
            Ok(None::<Vec<usize>>)
        ),
        test_empty_graph: (
            vec![],
            0,
            Err(FindHamiltonianCycleError::EmptyAdjacencyMatrix)
        ),
        test_improper_graph: (
            vec![
                vec![false, true],
                vec![true],
                vec![false, true, true],
                vec![true, true, true, false]
            ],
            0,
            Err(FindHamiltonianCycleError::ImproperAdjacencyMatrix)
        ),
        test_start_out_of_bound: (
            vec![
                vec![false, true, true],
                vec![true, false, true],
                vec![true, true, false],
            ],
            3,
            Err(FindHamiltonianCycleError::StartOutOfBound)
        ),
        test_complex_directed_graph: (
            vec![
                vec![false, true, false, true, false, false],
                vec![false, false, true, false, true, false],
                vec![false, false, false, true, false, false],
                vec![false, true, false, false, true, false],
                vec![false, false, true, false, false, true],
                vec![true, false, false, false, false, false],
            ],
            0,
            Ok(Some(vec![0, 1, 2, 3, 4, 5, 0]))
        ),
        single_node_self_loop: (
            vec![
                vec![true],
            ],
            0,
            Ok(Some(vec![0, 0]))
        ),
        single_node: (
            vec![
                vec![false],
            ],
            0,
            Ok(None)
        ),
    }
}
