//! Detection algorithms module for concurrency issues in Rust programs.
//!
//! This module contains various static analysis algorithms for detecting different types
//! of concurrency issues and safety violations in Rust programs using Petri net models
//! and state space exploration.
//!
//! ## Supported Detection Types
//!
//! ### Deadlock Detection (`deadlock`)
//! - Detects potential deadlocks in multi-threaded programs
//! - Analyzes lock acquisition patterns and circular dependencies
//! - Uses reachability analysis on Petri net state graphs
//! - Supports various locking primitives (Mutex, RwLock, etc.)
//!
//! ### Data Race Detection (`datarace`) 
//! - Identifies potential data races in concurrent memory access
//! - Analyzes unsafe memory operations and synchronization patterns
//! - Detects conflicting read/write operations on shared data
//! - Integrates with alias analysis for precise memory modeling
//!
//! ### Atomicity Violation Detection (`atomicity_violation`)
//! - Detects violations of atomic operation assumptions
//! - Analyzes atomic variable access patterns and memory ordering
//! - Identifies scenarios where atomic operations may not provide expected guarantees
//! - Supports various atomic operations (load, store, compare-exchange, etc.)
//!
//! ## Common Architecture
//!
//! All detectors follow a similar pattern:
//! 1. **Model Construction**: Build Petri net models from program analysis
//! 2. **State Exploration**: Generate and explore reachable program states
//! 3. **Pattern Detection**: Apply algorithm-specific detection logic
//! 4. **Report Generation**: Produce detailed analysis reports with source locations
//!
//! The detectors integrate with the broader analysis framework including:
//! - Call graph analysis for inter-procedural analysis
//! - Alias analysis for memory relationship tracking
//! - MIR analysis for low-level program representation
//! - Petri net modeling for concurrent behavior representation

pub mod atomicity_violation;
pub mod datarace;
pub mod deadlock;
