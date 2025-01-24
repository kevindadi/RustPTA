extern crate rustc_hash;
extern crate rustc_middle;

use rustc_hash::FxHashMap;
use rustc_middle::mir::{Body, Local, PlaceElem};
use rustc_middle::ty::{self, EarlyBinder, Instance, TyCtxt, TyKind, TypingEnv};
use rustc_span::Span;

use crate::graph::callgraph::InstanceId;
use crate::memory::pointsto::AliasId;

use serde_json::json;

/// 标识 channel 的类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelType {
    /// 无界 channel
    Mpsc,
    /// 有界 channel
    Sync(usize), // 包含缓冲区信息->对应库所容量
}

/// Channel 端点的类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointType {
    Sender,
    Receiver,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ChannelId {
    pub instance_id: InstanceId,
    pub local: Local,
}

impl<'tcx> ChannelId {
    pub fn new(instance_id: InstanceId, local: Local) -> Self {
        Self { instance_id, local }
    }

    // pub fn with_projection(
    //     instance_id: InstanceId,
    //     local: Local,
    //     projection: Vec<PlaceElem<'tcx>>,
    // ) -> Self {
    //     Self {
    //         instance_id,
    //         local,
    //         projection,
    //     }
    // }

    pub fn get_alias_id(&self) -> AliasId {
        AliasId::new(self.instance_id, self.local)
    }
}

#[derive(Debug)]
pub enum ChannelResult<'tcx> {
    // 单个channel端点
    Single(ChannelType, EndpointType, ty::Ty<'tcx>),
    // 成对的sender和receiver
    Pair(
        (ChannelType, EndpointType, ty::Ty<'tcx>),
        (ChannelType, EndpointType, ty::Ty<'tcx>),
    ),
}

/// Channel 的详细信息
#[derive(Debug, Clone)]
pub struct ChannelInfo<'tcx> {
    pub channel_type: ChannelType,
    pub endpoint_type: EndpointType,
    pub data_type: ty::Ty<'tcx>,
    pub span: Span,
}

impl<'tcx> ChannelInfo<'tcx> {
    pub fn new(
        channel_type: ChannelType,
        endpoint_type: EndpointType,
        data_type: ty::Ty<'tcx>,
        span: Span,
    ) -> Self {
        Self {
            channel_type,
            endpoint_type,
            data_type,
            span,
        }
    }
}

pub type ChannelMap<'tcx> = FxHashMap<ChannelId, ChannelInfo<'tcx>>;
pub type ChannelTuple<'tcx> = FxHashMap<ChannelId, (ChannelInfo<'tcx>, ChannelInfo<'tcx>)>;

/// Channel 信息收集器
pub struct ChannelCollector<'a, 'b, 'tcx> {
    instance_id: InstanceId,
    instance: &'a Instance<'tcx>,
    body: &'b Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    pub channels: ChannelMap<'tcx>,
    pub channel_tuples: ChannelTuple<'tcx>,
}

impl<'a, 'b, 'tcx> ChannelCollector<'a, 'b, 'tcx> {
    pub fn new(
        instance_id: InstanceId,
        instance: &'a Instance<'tcx>,
        body: &'b Body<'tcx>,
        tcx: TyCtxt<'tcx>,
    ) -> Self {
        Self {
            instance_id,
            instance,
            body,
            tcx,
            channels: Default::default(),
            channel_tuples: Default::default(),
        }
    }

    pub fn analyze(&mut self) {
        for (local, local_decl) in self.body.local_decls.iter_enumerated() {
            let typing_env = TypingEnv::post_analysis(self.tcx, self.instance.def_id());
            let local_ty = self.instance.instantiate_mir_and_normalize_erasing_regions(
                self.tcx,
                typing_env,
                EarlyBinder::bind(local_decl.ty),
            );

            if let Some(channel_result) = self.get_channel_info(local_ty) {
                match channel_result {
                    ChannelResult::Single(channel_type, endpoint_type, data_type) => {
                        let channel_id = ChannelId::new(self.instance_id, local);
                        let channel_info = ChannelInfo::new(
                            channel_type,
                            endpoint_type,
                            data_type,
                            local_decl.source_info.span,
                        );
                        self.channels.insert(channel_id, channel_info);
                    }
                    ChannelResult::Pair(sender_info, receiver_info) => {
                        // 创建sender的channel信息
                        let sender_id = ChannelId::new(self.instance_id, local);
                        let sender_channel_info = ChannelInfo::new(
                            sender_info.0,
                            sender_info.1,
                            sender_info.2,
                            local_decl.source_info.span,
                        );

                        // 创建receiver的channel信息（local + 1）
                        let receiver_id = ChannelId::new(self.instance_id, local);
                        let receiver_channel_info = ChannelInfo::new(
                            receiver_info.0,
                            receiver_info.1,
                            receiver_info.2,
                            local_decl.source_info.span,
                        );

                        // 保存到channels map
                        self.channels.insert(sender_id, sender_channel_info.clone());
                        self.channels
                            .insert(receiver_id, receiver_channel_info.clone());

                        // 保存配对信息
                        self.channel_tuples.push((
                            (sender_id, sender_channel_info),
                            (receiver_id, receiver_channel_info),
                        ));
                    }
                }
            }
        }
    }

    fn get_channel_info(&self, ty: ty::Ty<'tcx>) -> Option<ChannelResult<'tcx>> {
        match ty.kind() {
            TyKind::Adt(adt_def, substs) => {
                let path = self.tcx.def_path_str(adt_def.did());
                if path.contains("std::sync::mpsc") {
                    let data_type = substs.types().next()?;

                    if path.contains("Sender") {
                        Some(ChannelResult::Single(
                            ChannelType::Mpsc,
                            EndpointType::Sender,
                            data_type,
                        ))
                    } else if path.contains("SyncSender") {
                        Some(ChannelResult::Single(
                            ChannelType::Sync(0),
                            EndpointType::Sender,
                            data_type,
                        ))
                    } else if path.contains("Receiver") {
                        Some(ChannelResult::Single(
                            ChannelType::Mpsc,
                            EndpointType::Receiver,
                            data_type,
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            // 处理元组类型，用于 mpsc::channel() 的返回值
            TyKind::Tuple(types) => {
                if types.len() == 2 {
                    let sender_ty = types[0];
                    let receiver_ty = types[1];

                    if let (TyKind::Adt(sender_def, sender_substs), TyKind::Adt(receiver_def, _)) =
                        (sender_ty.kind(), receiver_ty.kind())
                    {
                        let sender_path = self.tcx.def_path_str(sender_def.did());
                        let receiver_path = self.tcx.def_path_str(receiver_def.did());

                        if sender_path.contains("std::sync::mpsc::Sender")
                            && receiver_path.contains("std::sync::mpsc::Receiver")
                        {
                            let data_type = sender_substs.types().next()?;
                            Some(ChannelResult::Pair(
                                (ChannelType::Mpsc, EndpointType::Sender, data_type),
                                (ChannelType::Mpsc, EndpointType::Receiver, data_type),
                            ))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// 将收集到的 channel 信息格式化输出为 JSON 格式
    #[allow(dead_code)]
    pub fn to_json_pretty(&self) -> Result<(), serde_json::Error> {
        if self.channels.is_empty() {
            log::debug!("No channels found");
        } else {
            for (channel_id, info) in self.channels.iter() {
                let channel_info = json!({
                    "location": {
                        "instance": self.tcx.def_path_str(self.instance.def_id()),
                        "local": channel_id.local.index(),
                    },
                    "channel_type": match info.channel_type {
                        ChannelType::Mpsc => "Mpsc".to_string(),
                        ChannelType::Sync(capacity) => {
                            format!("Sync({})", capacity)
                        }
                    },
                    "endpoint_type": format!("{:?}", info.endpoint_type),
                    "data_type": info.data_type.to_string(),
                    "defined_at": format!("{:?}", info.span),
                });

                log::info!(
                    "Channel Info:\n{}",
                    serde_json::to_string_pretty(&channel_info).unwrap()
                );
            }
        }
        Ok(())
    }

    /// 简单的文本格式输出
    pub fn print_debug_info(&self) {
        if self.channels.is_empty() {
            log::debug!("No channels found");
            return;
        }

        log::debug!("Found {} channels:", self.channels.len());
        for (channel_id, info) in self.channels.iter() {
            log::debug!(
                "Channel in function: {}",
                self.tcx.def_path_str(self.instance.def_id())
            );
            log::debug!("  Local ID: {}", channel_id.local.index());
            log::debug!("  Type: {:?}", info.channel_type);
            log::debug!("  Endpoint: {:?}", info.endpoint_type);
            log::debug!("  Data Type: {}", info.data_type);
            log::debug!("  Defined at: {:?}", info.span);
            log::debug!("----------------------------------------");
        }
    }
}
