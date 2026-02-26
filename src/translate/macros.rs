//! 转换宏
//!
//! - `transition_name!`: 统一变迁/库所命名格式
//! - `bb_place!`: 创建 BasicBlock 类型库所
//! - `add_fallthrough_transition!`: 创建 fallthrough 变迁并连接弧
//! - `add_terminal_transition!`: 创建 terminal 变迁并连接弧
//! - `add_wait_ret_subnet!`: 创建 wait place + ret transition 子网

/// 生成 `{name}_{bb_idx}_{kind}` 格式的变迁/库所名.
/// 可选后缀: `transition_name!(name, bb_idx, kind, suffix)`.
#[macro_export]
macro_rules! transition_name {
    ($name:expr, $bb_idx:expr, $kind:expr) => {
        format!("{}_{}_{}", $name, $bb_idx.index(), $kind)
    };
    ($name:expr, $bb_idx:expr, $kind:expr, $suffix:expr) => {
        format!("{}_{}_{}{}", $name, $bb_idx.index(), $kind, $suffix)
    };
}

/// 创建 BasicBlock 类型库所 (tokens=0, capacity=1).
#[macro_export]
macro_rules! bb_place {
    ($net:expr, $name:expr, $span:expr) => {{
        let place = $crate::net::Place::new(
            $name,
            0,
            1,
            $crate::net::structure::PlaceType::BasicBlock,
            $span.into(),
        );
        $net.add_place(place)
    }};
}

/// 创建 fallthrough 变迁并连接 last(bb_idx) -> t -> target.
/// 返回 TransitionId.
#[macro_export]
macro_rules! add_fallthrough_transition {
    ($self:expr, $bb_idx:expr, $name:expr, $kind:expr, $trans_type:expr, $target:expr) => {{
        let t_name = $crate::transition_name!($name, $bb_idx, $kind);
        let t = $crate::net::Transition::new_with_transition_type(t_name, $trans_type);
        let t_id = $self.net.add_transition(t);
        $self
            .net
            .add_input_arc($self.bb_graph.last($bb_idx), t_id, 1);
        $self
            .net
            .add_output_arc($self.bb_graph.start(*$target), t_id, 1);
        t_id
    }};
}

/// 创建 terminal 变迁并连接 last(bb_idx) -> t -> entry_exit.1.
/// 返回 TransitionId.
#[macro_export]
macro_rules! add_terminal_transition {
    ($self:expr, $bb_idx:expr, $name:expr, $kind:expr, $trans_type:expr) => {{
        let t_name = $crate::transition_name!($name, $bb_idx, $kind);
        let t = $crate::net::Transition::new_with_transition_type(t_name, $trans_type);
        let t_id = $self.net.add_transition(t);
        $self
            .net
            .add_input_arc($self.bb_graph.last($bb_idx), t_id, 1);
        $self.net.add_output_arc($self.entry_exit.1, t_id, 1);
        t_id
    }};
}

/// 创建 wait place + ret transition 子网,连接 wait -> bb_end, wait -> ret.
/// 参数: self, name, bb_idx, kind_wait, kind_ret, trans_type, span, bb_end(TransitionId).
/// 返回 (PlaceId, TransitionId).
#[macro_export]
macro_rules! add_wait_ret_subnet {
    ($self:expr, $name:expr, $bb_idx:expr, $kind_wait:expr, $kind_ret:expr, $trans_type:expr, $span:expr, $bb_end:expr) => {{
        let wait_name = $crate::transition_name!($name, $bb_idx, $kind_wait);
        let wait_place = $crate::bb_place!($self.net, wait_name, $span);
        let ret_name = $crate::transition_name!($name, $bb_idx, $kind_ret);
        let ret_t = $crate::net::Transition::new_with_transition_type(ret_name, $trans_type);
        let ret_id = $self.net.add_transition(ret_t);
        $self.net.add_output_arc(wait_place, $bb_end, 1);
        $self.net.add_input_arc(wait_place, ret_id, 1);
        (wait_place, ret_id)
    }};
}
