//! # Petri 网核心定义（Place/Transition Net）
//!
//! 设离散库所集合 `P` 与迁移集合 `T`，基数分别为 `|P|` 与 `|T|`。
//! 定义输入/输出映射 `Pre, Post ∈ ℕ^{|P|×|T|}`，以及迁移效应矩阵
//! `C = Post - Pre`。对任意标识 `M ∈ ℕ^{|P|}`：
//!
//! * 迁移 `t ∈ T` **可激发** 当且仅当满足：
//!   1. `∀p ∈ P: M[p] ≥ Pre[p, t]`；
//!   2. 若启用抑制弧（`feature = "inhibitor"`），则对所有抑制弧 `(p, t)` 有
//!      `M[p] < θ[p, t]`，其中 `θ[p, t]` 由 `Pre[p, t]` 给出；
//! * 迁移 **发射** 后标识满足 `M' = M + C[:, t]` 并遵循复位弧规则：若存在 `(p, t)`
//!   为复位弧（`feature = "reset"`），则发射后强制 `M'[p] = 0`。
//!
//! 提供的核心 API 支持：
//! * 可激发集计算、单步与随机发射；
//! * 并发冲突检测与顺序调度（随机选择）；
//! * 可达图构造、有界性与死锁分析、P/T 不变量（`feature = "invariants"`）；
//! * JSON/RON 序列化，Criterion 基准（`feature = "bench"`）。
//!
//! ## 示例
//!
//! ```rust
//! use RustPTA::net::*;
//!
//! let mut net = Net::empty();
//! let p0 = net.add_place(Place::new_with_tokens_and_capacity("p0", 1, 1));
//! let p1 = net.add_place(Place::new_with_tokens_and_capacity("p1", 0, 1));
//! let t0 = net.add_transition(Transition::new("t0"));
//!
//! net.set_input_weight(p0, t0, 1);
//! net.set_output_weight(p1, t0, 1);
//!
//! let marking = net.initial_marking();
//! assert_eq!(net.enabled_transitions(&marking), vec![t0]);
//! let next = net.fire_transition(&marking, t0).unwrap();
//! assert_eq!(next.tokens(p0), 0);
//! assert_eq!(next.tokens(p1), 1);
//! ```

pub mod core;
pub mod ids;
pub mod incidence;
pub mod index_vec;
pub mod io;
pub mod structure;

pub use core::{FireError, Net, ReachabilityEdge, ReachabilityGraph};
pub use ids::{PlaceId, TransitionId};
pub use incidence::{Incidence, IncidenceBool};
pub use index_vec::{Idx, IndexVec};
pub use structure::{Arc, ArcDirection, Marking, Place, Transition, TransitionType, Weight};
