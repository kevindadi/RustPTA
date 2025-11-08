use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt::Debug;









pub trait Chromosome<Rng: rand::Rng, Eval> {
    
    fn mutate(&mut self, rng: &mut Rng);

    
    fn crossover(&self, other: &Self, rng: &mut Rng) -> Self;

    
    
    fn fitness(&self) -> Eval;
}

pub trait SelectionStrategy<Rng: rand::Rng> {
    fn new(rng: Rng) -> Self;

    
    
    
    fn select<'a, Eval: Into<f64>, C: Chromosome<Rng, Eval>>(
        &mut self,
        population: &'a [C],
    ) -> (&'a C, &'a C);
}



pub struct RouletteWheel<Rng: rand::Rng> {
    rng: Rng,
}
impl<Rng: rand::Rng> SelectionStrategy<Rng> for RouletteWheel<Rng> {
    fn new(rng: Rng) -> Self {
        Self { rng }
    }

    fn select<'a, Eval: Into<f64>, C: Chromosome<Rng, Eval>>(
        &mut self,
        population: &'a [C],
    ) -> (&'a C, &'a C) {
        
        
        
        let mut parents = Vec::with_capacity(2);
        let fitnesses: Vec<f64> = population
            .iter()
            .filter_map(|individual| {
                let fitness = individual.fitness().into();
                if individual.fitness().into() == 0.0 {
                    parents.push(individual);
                    None
                } else {
                    Some(1.0 / fitness)
                }
            })
            .collect();
        if parents.len() == 2 {
            return (parents[0], parents[1]);
        }
        let sum: f64 = fitnesses.iter().sum();
        let mut spin = self.rng.gen_range(0.0..=sum);
        for individual in population {
            let fitness: f64 = individual.fitness().into();
            if spin <= fitness {
                parents.push(individual);
                if parents.len() == 2 {
                    return (parents[0], parents[1]);
                }
            } else {
                spin -= fitness;
            }
        }
        panic!("Could not select parents");
    }
}

pub struct Tournament<const K: usize, Rng: rand::Rng> {
    rng: Rng,
}
impl<const K: usize, Rng: rand::Rng> SelectionStrategy<Rng> for Tournament<K, Rng> {
    fn new(rng: Rng) -> Self {
        Self { rng }
    }

    fn select<'a, Eval, C: Chromosome<Rng, Eval>>(
        &mut self,
        population: &'a [C],
    ) -> (&'a C, &'a C) {
        if K < 2 {
            panic!("K must be > 2");
        }
        
        
        
        let mut picked_indices = BTreeSet::new(); 
        while picked_indices.len() < K {
            picked_indices.insert(self.rng.gen_range(0..population.len()));
        }
        let mut iter = picked_indices.into_iter();
        (
            &population[iter.next().unwrap()],
            &population[iter.next().unwrap()],
        )
    }
}

type Comparator<T> = Box<dyn FnMut(&T, &T) -> Ordering>;
pub struct GeneticAlgorithm<
    Rng: rand::Rng,
    Eval: PartialOrd,
    C: Chromosome<Rng, Eval>,
    Selection: SelectionStrategy<Rng>,
> {
    rng: Rng, 
    population: Vec<C>, 
    threshold: Eval, 
    max_generations: usize, 
    mutation_chance: f64, 
    crossover_chance: f64, 
    compare: Comparator<Eval>,
    selection: Selection, 
}

pub struct GenericAlgorithmParams {
    max_generations: usize,
    mutation_chance: f64,
    crossover_chance: f64,
}

impl<
    Rng: rand::Rng,
    Eval: Into<f64> + PartialOrd + Debug,
    C: Chromosome<Rng, Eval> + Clone + Debug,
    Selection: SelectionStrategy<Rng>,
> GeneticAlgorithm<Rng, Eval, C, Selection>
{
    pub fn init(
        rng: Rng,
        population: Vec<C>,
        threshold: Eval,
        params: GenericAlgorithmParams,
        compare: Comparator<Eval>,
        selection: Selection,
    ) -> Self {
        let GenericAlgorithmParams {
            max_generations,
            mutation_chance,
            crossover_chance,
        } = params;
        Self {
            rng,
            population,
            threshold,
            max_generations,
            mutation_chance,
            crossover_chance,
            compare,
            selection,
        }
    }

    pub fn solve(&mut self) -> Option<C> {
        let mut generations = 1; 
        while generations <= self.max_generations {
            
            self.population
                .sort_by(|c1: &C, c2: &C| (self.compare)(&c1.fitness(), &c2.fitness()));

            
            if let Some(solution) = self.population.first() {
                if solution.fitness() <= self.threshold {
                    return Some(solution).cloned();
                }
            }

            
            for chromosome in self.population.iter_mut() {
                if self.rng.r#gen::<f64>() <= self.mutation_chance {
                    chromosome.mutate(&mut self.rng);
                }
            }
            
            let mut new_population = Vec::with_capacity(self.population.len() + 1);
            while new_population.len() < self.population.len() {
                let (p1, p2) = self.selection.select(&self.population);
                if self.rng.r#gen::<f64>() <= self.crossover_chance {
                    let child = p1.crossover(p2, &mut self.rng);
                    new_population.push(child);
                } else {
                    
                    new_population.extend([p1.clone(), p2.clone()]);
                }
            }
            if new_population.len() > self.population.len() {
                
                new_population.pop();
            }
            self.population = new_population;
            
            generations += 1;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::general::genetic::{
        Chromosome, GenericAlgorithmParams, GeneticAlgorithm, RouletteWheel, SelectionStrategy,
        Tournament,
    };
    use rand::rngs::ThreadRng;
    use rand::{Rng, thread_rng};
    use std::collections::HashMap;
    use std::fmt::{Debug, Formatter};
    use std::ops::RangeInclusive;

    #[test]
    #[ignore] 
    fn find_secret() {
        let chars = 'a'..='z';
        let secret = "thisistopsecret".to_owned();
        
        #[derive(Clone)]
        struct TestString {
            chars: RangeInclusive<char>,
            secret: String,
            genes: Vec<char>,
        }
        impl TestString {
            fn new(rng: &mut ThreadRng, secret: String, chars: RangeInclusive<char>) -> Self {
                let current = (0..secret.len())
                    .map(|_| rng.gen_range(chars.clone()))
                    .collect::<Vec<_>>();

                Self {
                    chars,
                    secret,
                    genes: current,
                }
            }
        }
        impl Debug for TestString {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.genes.iter().collect::<String>())
            }
        }
        impl Chromosome<ThreadRng, i32> for TestString {
            fn mutate(&mut self, rng: &mut ThreadRng) {
                
                let gene_idx = rng.gen_range(0..self.secret.len());
                let new_char = rng.gen_range(self.chars.clone());
                self.genes[gene_idx] = new_char;
            }

            fn crossover(&self, other: &Self, rng: &mut ThreadRng) -> Self {
                
                let genes = (0..self.secret.len())
                    .map(|idx| {
                        if rng.gen_bool(0.5) {
                            
                            self.genes[idx]
                        } else {
                            
                            other.genes[idx]
                        }
                    })
                    .collect();
                Self {
                    chars: self.chars.clone(),
                    secret: self.secret.clone(),
                    genes,
                }
            }

            fn fitness(&self) -> i32 {
                
                self.genes
                    .iter()
                    .zip(self.secret.chars())
                    .filter(|(char, expected)| expected != *char)
                    .count() as i32
            }
        }
        let mut rng = thread_rng();
        let pop_count = 1_000;
        let mut population = Vec::with_capacity(pop_count);
        for _ in 0..pop_count {
            population.push(TestString::new(&mut rng, secret.clone(), chars.clone()));
        }
        let selection: Tournament<100, ThreadRng> = Tournament::new(rng.clone());
        let params = GenericAlgorithmParams {
            max_generations: 100,
            mutation_chance: 0.2,
            crossover_chance: 0.4,
        };
        let mut solver =
            GeneticAlgorithm::init(rng, population, 0, params, Box::new(i32::cmp), selection);
        let res = solver.solve();
        assert!(res.is_some());
        assert_eq!(res.unwrap().genes, secret.chars().collect::<Vec<_>>())
    }

    #[test]
    #[ignore] 
    fn solve_mastermind() {
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
        enum ColoredPeg {
            Red,
            Yellow,
            Green,
            Blue,
            White,
            Black,
        }
        struct GuessAnswer {
            right_pos: i32, 
            wrong_pos: i32, 
        }
        #[derive(Clone, Debug)]
        struct CodeMaker {
            
            code: [ColoredPeg; 4],
            count_by_color: HashMap<ColoredPeg, usize>,
        }
        impl CodeMaker {
            fn new(code: [ColoredPeg; 4]) -> Self {
                let mut count_by_color = HashMap::with_capacity(4);
                for peg in &code {
                    *count_by_color.entry(*peg).or_insert(0) += 1;
                }
                Self {
                    code,
                    count_by_color,
                }
            }
            fn eval(&self, guess: &[ColoredPeg; 4]) -> GuessAnswer {
                let mut right_pos = 0;
                let mut wrong_pos = 0;
                let mut idx_by_colors = self.count_by_color.clone();
                for (idx, color) in guess.iter().enumerate() {
                    if self.code[idx] == *color {
                        right_pos += 1;
                        let count = idx_by_colors.get_mut(color).unwrap();
                        *count -= 1; 
                        if *count == 0 {
                            idx_by_colors.remove(color);
                        }
                    }
                }
                for (idx, color) in guess.iter().enumerate() {
                    if self.code[idx] != *color {
                        
                        if let Some(count) = idx_by_colors.get_mut(color) {
                            *count -= 1;
                            if *count == 0 {
                                idx_by_colors.remove(color);
                            }
                            wrong_pos += 1;
                        }
                    }
                }
                GuessAnswer {
                    right_pos,
                    wrong_pos,
                }
            }
        }

        #[derive(Clone)]
        struct CodeBreaker {
            maker: CodeMaker, 
            guess: [ColoredPeg; 4],
        }
        impl Debug for CodeBreaker {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.write_str(format!("{:?}", self.guess).as_str())
            }
        }
        fn random_color(rng: &mut ThreadRng) -> ColoredPeg {
            match rng.gen_range(0..=5) {
                0 => ColoredPeg::Red,
                1 => ColoredPeg::Yellow,
                2 => ColoredPeg::Green,
                3 => ColoredPeg::Blue,
                4 => ColoredPeg::White,
                _ => ColoredPeg::Black,
            }
        }
        fn random_guess(rng: &mut ThreadRng) -> [ColoredPeg; 4] {
            std::array::from_fn(|_| random_color(rng))
        }
        impl Chromosome<ThreadRng, i32> for CodeBreaker {
            fn mutate(&mut self, rng: &mut ThreadRng) {
                
                let idx = rng.gen_range(0..4);
                self.guess[idx] = random_color(rng);
            }

            fn crossover(&self, other: &Self, rng: &mut ThreadRng) -> Self {
                Self {
                    maker: self.maker.clone(),
                    guess: std::array::from_fn(|i| {
                        if rng.r#gen::<f64>() < 0.5 {
                            self.guess[i]
                        } else {
                            other.guess[i]
                        }
                    }),
                }
            }

            fn fitness(&self) -> i32 {
                
                let answer = self.maker.eval(&self.guess);
                
                let mut res = 32; 
                res -= answer.right_pos * 8; 
                res -= answer.wrong_pos; 
                res
            }
        }
        let code = [
            ColoredPeg::Red,
            ColoredPeg::Red,
            ColoredPeg::White,
            ColoredPeg::Blue,
        ];
        let maker = CodeMaker::new(code);
        let population_count = 10;
        let params = GenericAlgorithmParams {
            max_generations: 100,
            mutation_chance: 0.5,
            crossover_chance: 0.3,
        };
        let mut rng = thread_rng();
        let mut initial_pop = Vec::with_capacity(population_count);
        for _ in 0..population_count {
            initial_pop.push(CodeBreaker {
                maker: maker.clone(),
                guess: random_guess(&mut rng),
            });
        }
        let selection = RouletteWheel { rng: rng.clone() };
        let mut solver =
            GeneticAlgorithm::init(rng, initial_pop, 0, params, Box::new(i32::cmp), selection);
        let res = solver.solve();
        assert!(res.is_some());
        assert_eq!(code, res.unwrap().guess);
    }
}
