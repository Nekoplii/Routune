use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use rubato::{FastFixedIn, PolynomialDegree, Resampler};
use serde::{Deserialize, Serialize};

pub const DEFAULT_MAX_CHUNKS: usize = 4096;
pub const MAX_GRAPH_NODES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessContext {
    pub sample_rate: u32,
    pub channels: usize,
    pub frames: usize,
}

impl Default for ProcessContext {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            frames: 256,
        }
    }
}

#[inline]
pub fn calculate_metrics(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (-100.0, -100.0);
    }
    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f32;
    for &sample in samples {
        let abs = sample.abs();
        if abs > peak {
            peak = abs;
        }
        sum_sq += sample * sample;
    }
    let rms = (sum_sq / samples.len() as f32).sqrt();

    let peak_db = if peak > 1e-5 {
        20.0 * peak.log10()
    } else {
        -100.0
    };

    let rms_db = if rms > 1e-5 {
        20.0 * rms.log10()
    } else {
        -100.0
    };

    (peak_db, rms_db)
}

pub trait AudioNode: Send {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn node_type(&self) -> &str;
    fn is_bypassed(&self) -> bool {
        false
    }
    fn set_bypassed(&mut self, _bypassed: bool) {}
    fn process(&mut self, input: &[f32], output: &mut [f32], context: &ProcessContext);
    fn reset(&mut self) {}
    fn peak_db(&self) -> f32 {
        -100.0
    }
    fn rms_db(&self) -> f32 {
        -100.0
    }
    fn set_gain(&mut self, _gain: f32) {}
}

pub struct PipeWireInputNode {
    id: String,
    name: String,
    sink_name: String,
    bypassed: bool,
    gain: f32,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
}

impl PipeWireInputNode {
    pub fn new(id: impl Into<String>, name: impl Into<String>, sink_name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            sink_name: sink_name.into(),
            bypassed: false,
            gain: 1.0,
            peak_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            rms_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
        }
    }

    pub fn sink_name(&self) -> &str {
        &self.sink_name
    }

    pub fn stream_name(&self) -> &str {
        &self.sink_name
    }

    pub fn metrics_handles(&self) -> (Arc<AtomicU32>, Arc<AtomicU32>) {
        (self.peak_db.clone(), self.rms_db.clone())
    }
}

impl AudioNode for PipeWireInputNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "PipeWireInputNode"
    }

    fn is_bypassed(&self) -> bool {
        self.bypassed
    }

    fn set_bypassed(&mut self, bypassed: bool) {
        self.bypassed = bypassed;
    }

    fn set_gain(&mut self, gain: f32) {
        self.gain = gain;
    }

    fn process(&mut self, input: &[f32], output: &mut [f32], _context: &ProcessContext) {
        let (peak, rms) = calculate_metrics(input);
        self.peak_db.store(peak.to_bits(), Ordering::Relaxed);
        self.rms_db.store(rms.to_bits(), Ordering::Relaxed);

        if self.bypassed {
            output.fill(0.0);
        } else {
            let len = input.len().min(output.len());
            if (self.gain - 1.0).abs() < 1e-6 {
                output[..len].copy_from_slice(&input[..len]);
            } else {
                for i in 0..len {
                    output[i] = input[i] * self.gain;
                }
            }
            if len < output.len() {
                output[len..].fill(0.0);
            }
        }
    }

    fn peak_db(&self) -> f32 {
        f32::from_bits(self.peak_db.load(Ordering::Relaxed))
    }

    fn rms_db(&self) -> f32 {
        f32::from_bits(self.rms_db.load(Ordering::Relaxed))
    }
}

pub type VirtualInputNode = PipeWireInputNode;

pub struct DspBypassNode {
    id: String,
    name: String,
    gain: f32,
    bypassed: bool,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
}

impl DspBypassNode {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            gain: 1.0,
            bypassed: false,
            peak_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            rms_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
        }
    }

    pub fn metrics_handles(&self) -> (Arc<AtomicU32>, Arc<AtomicU32>) {
        (self.peak_db.clone(), self.rms_db.clone())
    }
}

impl AudioNode for DspBypassNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "DspBypassNode"
    }

    fn is_bypassed(&self) -> bool {
        self.bypassed
    }

    fn set_bypassed(&mut self, bypassed: bool) {
        self.bypassed = bypassed;
    }

    fn set_gain(&mut self, gain: f32) {
        self.gain = gain;
    }

    fn process(&mut self, input: &[f32], output: &mut [f32], _context: &ProcessContext) {
        let len = input.len().min(output.len());
        if self.bypassed {
            output[..len].copy_from_slice(&input[..len]);
        } else {
            for i in 0..len {
                output[i] = input[i] * self.gain;
            }
        }

        let (peak, rms) = calculate_metrics(&output[..len]);
        self.peak_db.store(peak.to_bits(), Ordering::Relaxed);
        self.rms_db.store(rms.to_bits(), Ordering::Relaxed);
    }

    fn peak_db(&self) -> f32 {
        f32::from_bits(self.peak_db.load(Ordering::Relaxed))
    }

    fn rms_db(&self) -> f32 {
        f32::from_bits(self.rms_db.load(Ordering::Relaxed))
    }
}

pub struct ResamplerNode {
    id: String,
    name: String,
    source_rate: u32,
    target_rate: u32,
    channels: usize,
    chunk_size: usize,
    bypassed: bool,
    resampler: Option<FastFixedIn<f32>>,
    in_planar: Vec<Vec<f32>>,
    out_planar: Vec<Vec<f32>>,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
}

impl ResamplerNode {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        source_rate: u32,
        target_rate: u32,
        channels: usize,
        chunk_size: usize,
    ) -> Result<Self, rubato::ResamplerConstructionError> {
        let (resampler, in_planar, out_planar) = if source_rate != target_rate {
            let ratio = target_rate as f64 / source_rate as f64;
            let r = FastFixedIn::<f32>::new(
                ratio,
                1.0,
                PolynomialDegree::Linear,
                chunk_size,
                channels,
            )?;
            let max_out = r.output_frames_max();
            let in_buf = vec![vec![0.0f32; chunk_size]; channels];
            let out_buf = vec![vec![0.0f32; max_out]; channels];
            (Some(r), in_buf, out_buf)
        } else {
            (None, Vec::new(), Vec::new())
        };

        Ok(Self {
            id: id.into(),
            name: name.into(),
            source_rate,
            target_rate,
            channels,
            chunk_size,
            bypassed: false,
            resampler,
            in_planar,
            out_planar,
            peak_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            rms_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
        })
    }
}

impl AudioNode for ResamplerNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "ResamplerNode"
    }

    fn is_bypassed(&self) -> bool {
        self.bypassed
    }

    fn set_bypassed(&mut self, bypassed: bool) {
        self.bypassed = bypassed;
    }

    fn process(&mut self, input: &[f32], output: &mut [f32], _context: &ProcessContext) {
        if self.bypassed || self.source_rate == self.target_rate || self.resampler.is_none() {
            let len = input.len().min(output.len());
            output[..len].copy_from_slice(&input[..len]);
            let (peak, rms) = calculate_metrics(&output[..len]);
            self.peak_db.store(peak.to_bits(), Ordering::Relaxed);
            self.rms_db.store(rms.to_bits(), Ordering::Relaxed);
            return;
        }

        let resampler = self.resampler.as_mut().unwrap();
        let frames_in = (input.len() / self.channels).min(self.chunk_size);
        if frames_in == 0 {
            output.fill(0.0);
            return;
        }

        // De-interleave into pre-allocated planar input buffer
        for ch in 0..self.channels {
            for f in 0..frames_in {
                self.in_planar[ch][f] = input[f * self.channels + ch];
            }
        }

        // Resample in place without dynamic heap allocation
        if let Ok((_in_consumed, out_produced)) =
            resampler.process_into_buffer(&self.in_planar, &mut self.out_planar, None)
        {
            let max_frames_out = output.len() / self.channels;
            let frames_to_write = out_produced.min(max_frames_out);

            // Interleave back into output buffer
            for f in 0..frames_to_write {
                for ch in 0..self.channels {
                    output[f * self.channels + ch] = self.out_planar[ch][f];
                }
            }

            let total_written = frames_to_write * self.channels;
            if total_written < output.len() {
                output[total_written..].fill(0.0);
            }

            let (peak, rms) = calculate_metrics(&output[..total_written]);
            self.peak_db.store(peak.to_bits(), Ordering::Relaxed);
            self.rms_db.store(rms.to_bits(), Ordering::Relaxed);
        } else {
            output.fill(0.0);
        }
    }

    fn peak_db(&self) -> f32 {
        f32::from_bits(self.peak_db.load(Ordering::Relaxed))
    }

    fn rms_db(&self) -> f32 {
        f32::from_bits(self.rms_db.load(Ordering::Relaxed))
    }
}

pub struct PhysicalOutputNode {
    id: String,
    name: String,
    device_name: String,
    bypassed: bool,
    peak_db: Arc<AtomicU32>,
    rms_db: Arc<AtomicU32>,
    frames_processed: Arc<AtomicU64>,
}

impl PhysicalOutputNode {
    pub fn new(id: impl Into<String>, name: impl Into<String>, device_name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            device_name: device_name.into(),
            bypassed: false,
            peak_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            rms_db: Arc::new(AtomicU32::new((-100.0f32).to_bits())),
            frames_processed: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn metrics_handles(&self) -> (Arc<AtomicU32>, Arc<AtomicU32>, Arc<AtomicU64>) {
        (
            self.peak_db.clone(),
            self.rms_db.clone(),
            self.frames_processed.clone(),
        )
    }
}

impl AudioNode for PhysicalOutputNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "PhysicalOutputNode"
    }

    fn is_bypassed(&self) -> bool {
        self.bypassed
    }

    fn set_bypassed(&mut self, bypassed: bool) {
        self.bypassed = bypassed;
    }

    fn process(&mut self, input: &[f32], output: &mut [f32], context: &ProcessContext) {
        let len = input.len().min(output.len());
        if self.bypassed {
            output.fill(0.0);
        } else {
            output[..len].copy_from_slice(&input[..len]);
        }

        let (peak, rms) = calculate_metrics(&output[..len]);
        self.peak_db.store(peak.to_bits(), Ordering::Relaxed);
        self.rms_db.store(rms.to_bits(), Ordering::Relaxed);

        if context.channels > 0 {
            let frames = len / context.channels;
            self.frames_processed.fetch_add(frames as u64, Ordering::Relaxed);
        }
    }

    fn peak_db(&self) -> f32 {
        f32::from_bits(self.peak_db.load(Ordering::Relaxed))
    }

    fn rms_db(&self) -> f32 {
        f32::from_bits(self.rms_db.load(Ordering::Relaxed))
    }
}

pub enum GraphCommand {
    AddNode(Box<dyn AudioNode>),
    RemoveNode(String),
    Connect { from: String, to: String },
    Disconnect { from: String, to: String },
    SetBypass { id: String, bypassed: bool },
    SetGain { id: String, gain: f32 },
}

pub struct NodeSlot {
    pub node: Box<dyn AudioNode>,
    pub input_buffer: Vec<f32>,
    pub output_buffer: Vec<f32>,
    pub targets: Vec<usize>,
}

pub struct AudioEngineGraph {
    slots: Vec<Option<NodeSlot>>,
    execution_order: Vec<usize>,
    cmd_consumer: rtrb::Consumer<GraphCommand>,
    recycle_producer: rtrb::Producer<Box<dyn AudioNode>>,
    scratch_mix: Vec<f32>,
}

pub type Graph = AudioEngineGraph;

impl AudioEngineGraph {
    pub fn new(
        cmd_consumer: rtrb::Consumer<GraphCommand>,
        recycle_producer: rtrb::Producer<Box<dyn AudioNode>>,
        max_buffer_size: usize,
    ) -> Self {
        let mut slots = Vec::with_capacity(MAX_GRAPH_NODES);
        for _ in 0..MAX_GRAPH_NODES {
            slots.push(None);
        }

        Self {
            slots,
            execution_order: Vec::with_capacity(MAX_GRAPH_NODES),
            cmd_consumer,
            recycle_producer,
            scratch_mix: vec![0.0f32; max_buffer_size],
        }
    }

    fn find_slot_by_id(&self, id: &str) -> Option<usize> {
        for (i, slot) in self.slots.iter().enumerate() {
            if let Some(s) = slot {
                if s.node.id() == id {
                    return Some(i);
                }
            }
        }
        None
    }

    fn find_free_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.is_none())
    }

    /// Recompute topological execution order of active nodes using Kahn's algorithm.
    fn recompute_execution_order(&mut self) {
        let mut in_degrees = [0usize; MAX_GRAPH_NODES];
        let mut active_indices = Vec::with_capacity(MAX_GRAPH_NODES);

        for (i, slot) in self.slots.iter().enumerate() {
            if slot.is_some() {
                active_indices.push(i);
            }
        }

        for &i in &active_indices {
            if let Some(slot) = &self.slots[i] {
                for &target in &slot.targets {
                    if target < MAX_GRAPH_NODES && self.slots[target].is_some() {
                        in_degrees[target] += 1;
                    }
                }
            }
        }

        let mut queue = Vec::with_capacity(MAX_GRAPH_NODES);
        for &i in &active_indices {
            if in_degrees[i] == 0 {
                queue.push(i);
            }
        }

        let mut sorted = Vec::with_capacity(active_indices.len());
        while let Some(u) = queue.pop() {
            sorted.push(u);
            if let Some(slot) = &self.slots[u] {
                for &v in &slot.targets {
                    if v < MAX_GRAPH_NODES && self.slots[v].is_some() {
                        in_degrees[v] = in_degrees[v].saturating_sub(1);
                        if in_degrees[v] == 0 {
                            queue.push(v);
                        }
                    }
                }
            }
        }

        // If cycle detected, append remaining active nodes
        for &i in &active_indices {
            if !sorted.contains(&i) {
                sorted.push(i);
            }
        }

        self.execution_order = sorted;
    }

    pub fn drain_commands(&mut self) {
        let mut topology_changed = false;
        while let Ok(cmd) = self.cmd_consumer.pop() {
            match cmd {
                GraphCommand::AddNode(node) => {
                    let id = node.id().to_string();
                    if self.find_slot_by_id(&id).is_some() {
                        let _ = self.recycle_producer.push(node);
                        continue;
                    }
                    if let Some(free_idx) = self.find_free_slot() {
                        self.slots[free_idx] = Some(NodeSlot {
                            node,
                            input_buffer: vec![0.0f32; self.scratch_mix.len()],
                            output_buffer: vec![0.0f32; self.scratch_mix.len()],
                            targets: Vec::with_capacity(MAX_GRAPH_NODES),
                        });
                        topology_changed = true;
                    } else {
                        let _ = self.recycle_producer.push(node);
                    }
                }
                GraphCommand::RemoveNode(id) => {
                    if let Some(idx) = self.find_slot_by_id(&id) {
                        for slot in self.slots.iter_mut().flatten() {
                            slot.targets.retain(|&t| t != idx);
                        }
                        if let Some(removed_slot) = self.slots[idx].take() {
                            let _ = self.recycle_producer.push(removed_slot.node);
                        }
                        topology_changed = true;
                    }
                }
                GraphCommand::Connect { from, to } => {
                    if let (Some(from_idx), Some(to_idx)) =
                        (self.find_slot_by_id(&from), self.find_slot_by_id(&to))
                    {
                        if let Some(slot) = &mut self.slots[from_idx] {
                            if !slot.targets.contains(&to_idx) {
                                slot.targets.push(to_idx);
                                topology_changed = true;
                            }
                        }
                    }
                }
                GraphCommand::Disconnect { from, to } => {
                    if let (Some(from_idx), Some(to_idx)) =
                        (self.find_slot_by_id(&from), self.find_slot_by_id(&to))
                    {
                        if let Some(slot) = &mut self.slots[from_idx] {
                            slot.targets.retain(|&t| t != to_idx);
                            topology_changed = true;
                        }
                    }
                }
                GraphCommand::SetBypass { id, bypassed } => {
                    if let Some(idx) = self.find_slot_by_id(&id) {
                        if let Some(slot) = &mut self.slots[idx] {
                            slot.node.set_bypassed(bypassed);
                        }
                    }
                }
                GraphCommand::SetGain { id, gain } => {
                    if let Some(idx) = self.find_slot_by_id(&id) {
                        if let Some(slot) = &mut self.slots[idx] {
                            slot.node.set_gain(gain);
                        }
                    }
                }
            }
        }

        if topology_changed {
            self.recompute_execution_order();
        }
    }

    /// Real-time audio rendering iteration.
    ///
    /// Must remain lock-free, zero allocation, and bounded.
    pub fn process_quantum(&mut self, final_output: &mut [f32], context: &ProcessContext) {
        self.drain_commands();

        let len = final_output.len().min(self.scratch_mix.len());
        self.scratch_mix[..len].fill(0.0);

        // Process in topological order so producer outputs are instantly available to consumers
        for &idx in &self.execution_order {
            if idx >= self.slots.len() || self.slots[idx].is_none() {
                continue;
            }

            // 1. Process slot
            let (targets, is_terminal) = {
                let slot = self.slots[idx].as_mut().unwrap();
                let in_len = slot.input_buffer.len().min(len);
                slot.node.process(
                    &slot.input_buffer[..in_len],
                    &mut slot.output_buffer[..in_len],
                    context,
                );
                slot.input_buffer[..in_len].fill(0.0);
                (slot.targets.clone(), slot.targets.is_empty())
            };

            // 2. Mix to target inputs or final scratch mix
            if is_terminal {
                if let Some(slot) = &self.slots[idx] {
                    for s in 0..len {
                        self.scratch_mix[s] += slot.output_buffer[s];
                    }
                }
            } else {
                for target_idx in targets {
                    if target_idx == idx || target_idx >= self.slots.len() {
                        continue;
                    }

                    let (first, second) = if idx < target_idx {
                        let (left, right) = self.slots.split_at_mut(target_idx);
                        (&left[idx], &mut right[0])
                    } else {
                        let (left, right) = self.slots.split_at_mut(idx);
                        (&right[0], &mut left[target_idx])
                    };

                    if let (Some(src), Some(dst)) = (first, second) {
                        let copy_len = len.min(src.output_buffer.len()).min(dst.input_buffer.len());
                        for s in 0..copy_len {
                            dst.input_buffer[s] += src.output_buffer[s];
                        }
                    }
                }
            }
        }

        final_output[..len].copy_from_slice(&self.scratch_mix[..len]);
    }

    /// Feed input directly into a designated node slot (e.g. from virtual sink).
    pub fn feed_node_input(&mut self, node_id: &str, input: &[f32]) {
        if let Some(idx) = self.find_slot_by_id(node_id) {
            if let Some(slot) = &mut self.slots[idx] {
                let len = input.len().min(slot.input_buffer.len());
                for i in 0..len {
                    slot.input_buffer[i] += input[i];
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub name: String,
    pub node_type: String,
    pub bypassed: bool,
}

pub struct GraphController {
    cmd_producer: rtrb::Producer<GraphCommand>,
    recycle_consumer: rtrb::Consumer<Box<dyn AudioNode>>,
    active_nodes: Vec<String>,
    active_edges: Vec<(String, String)>,
    node_infos: Vec<NodeInfo>,
}

impl GraphController {
    pub fn new(
        cmd_producer: rtrb::Producer<GraphCommand>,
        recycle_consumer: rtrb::Consumer<Box<dyn AudioNode>>,
    ) -> Self {
        Self {
            cmd_producer,
            recycle_consumer,
            active_nodes: Vec::new(),
            active_edges: Vec::new(),
            node_infos: Vec::new(),
        }
    }

    pub fn add_node(&mut self, node: Box<dyn AudioNode>) -> Result<(), &'static str> {
        let id = node.id().to_string();
        if self.active_nodes.contains(&id) {
            return Err("Node ID already exists");
        }
        let info = NodeInfo {
            id: id.clone(),
            name: node.name().to_string(),
            node_type: node.node_type().to_string(),
            bypassed: node.is_bypassed(),
        };
        self.cmd_producer
            .push(GraphCommand::AddNode(node))
            .map_err(|_| "Command queue full")?;
        self.active_nodes.push(id);
        self.node_infos.push(info);
        Ok(())
    }

    pub fn remove_node(&mut self, id: &str) -> Result<(), &'static str> {
        if !self.active_nodes.iter().any(|n| n == id) {
            return Err("Node ID not found");
        }
        self.cmd_producer
            .push(GraphCommand::RemoveNode(id.to_string()))
            .map_err(|_| "Command queue full")?;
        self.active_nodes.retain(|n| n != id);
        self.node_infos.retain(|n| n.id != id);
        self.active_edges
            .retain(|(from, to)| from != id && to != id);
        Ok(())
    }

    pub fn connect(&mut self, from: &str, to: &str) -> Result<(), &'static str> {
        let edge = (from.to_string(), to.to_string());
        if self.active_edges.contains(&edge) {
            return Ok(());
        }
        self.cmd_producer
            .push(GraphCommand::Connect {
                from: from.to_string(),
                to: to.to_string(),
            })
            .map_err(|_| "Command queue full")?;
        self.active_edges.push(edge);
        Ok(())
    }

    pub fn disconnect(&mut self, from: &str, to: &str) -> Result<(), &'static str> {
        self.cmd_producer
            .push(GraphCommand::Disconnect {
                from: from.to_string(),
                to: to.to_string(),
            })
            .map_err(|_| "Command queue full")?;
        self.active_edges
            .retain(|(f, t)| !(f == from && t == to));
        Ok(())
    }

    pub fn set_bypass(&mut self, id: &str, bypassed: bool) -> Result<(), &'static str> {
        self.cmd_producer
            .push(GraphCommand::SetBypass {
                id: id.to_string(),
                bypassed,
            })
            .map_err(|_| "Command queue full")?;
        if let Some(info) = self.node_infos.iter_mut().find(|n| n.id == id) {
            info.bypassed = bypassed;
        }
        Ok(())
    }

    pub fn set_gain(&mut self, id: &str, gain: f32) -> Result<(), &'static str> {
        self.cmd_producer
            .push(GraphCommand::SetGain {
                id: id.to_string(),
                gain,
            })
            .map_err(|_| "Command queue full")?;
        Ok(())
    }

    /// Drain and safely deallocate recycled nodes on the non-RT control thread.
    pub fn collect_garbage(&mut self) -> usize {
        let mut count = 0;
        while self.recycle_consumer.pop().is_ok() {
            count += 1;
        }
        count
    }

    pub fn active_nodes(&self) -> &[String] {
        &self.active_nodes
    }

    pub fn node_infos(&self) -> &[NodeInfo] {
        &self.node_infos
    }

    pub fn active_edges(&self) -> &[(String, String)] {
        &self.active_edges
    }
}

pub fn create_graph_pair(
    queue_capacity: usize,
    max_buffer_size: usize,
) -> (GraphController, AudioEngineGraph) {
    let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(queue_capacity);
    let (recycle_tx, recycle_rx) = rtrb::RingBuffer::new(queue_capacity);

    let controller = GraphController::new(cmd_tx, recycle_rx);
    let engine = AudioEngineGraph::new(cmd_rx, recycle_tx, max_buffer_size);

    (controller, engine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_input_node_process_and_telemetry() {
        let mut node = VirtualInputNode::new("game-input", "Game Audio", "routune-game");
        let ctx = ProcessContext::default();
        let input = [0.5f32, -0.5f32, 0.5f32, -0.5f32];
        let mut output = [0.0f32; 4];

        node.process(&input, &mut output, &ctx);

        assert_eq!(output, input);
        assert!((node.peak_db() - (-6.02)).abs() < 0.1);
        assert!((node.rms_db() - (-6.02)).abs() < 0.1);

        node.set_bypassed(true);
        node.process(&input, &mut output, &ctx);
        assert_eq!(output, [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_dsp_bypass_node_gain() {
        let mut node = DspBypassNode::new("dsp-gain", "Gain Stage");
        node.set_gain(0.5);
        let ctx = ProcessContext::default();
        let input = [1.0f32, -1.0f32];
        let mut output = [0.0f32; 2];

        node.process(&input, &mut output, &ctx);
        assert_eq!(output, [0.5, -0.5]);

        node.set_bypassed(true);
        node.process(&input, &mut output, &ctx);
        assert_eq!(output, [1.0, -1.0]);
    }

    #[test]
    fn test_resampler_node_pass_through_when_matching_rate() {
        let mut resampler = ResamplerNode::new(
            "resampler-1",
            "Resampler 48k to 48k",
            48000,
            48000,
            2,
            256,
        )
        .expect("Failed to create matching resampler");

        let ctx = ProcessContext::default();
        let input = [0.25f32; 16];
        let mut output = [0.0f32; 16];

        resampler.process(&input, &mut output, &ctx);
        assert_eq!(output, input);
    }

    #[test]
    fn test_resampler_node_dynamic_resample() {
        let chunk_size = 256;
        let mut resampler = ResamplerNode::new(
            "resampler-44k-48k",
            "Resampler 44.1k to 48k",
            44100,
            48000,
            2,
            chunk_size,
        )
        .expect("Failed to create resampler");

        let ctx = ProcessContext {
            sample_rate: 48000,
            channels: 2,
            frames: chunk_size,
        };

        let input = vec![0.5f32; chunk_size * 2];
        let mut output = vec![0.0f32; 600 * 2];

        resampler.process(&input, &mut output, &ctx);
        assert!(resampler.peak_db() > -10.0);
    }

    #[test]
    fn test_lock_free_graph_mutation_and_recycling() {
        let (mut controller, mut engine) = create_graph_pair(32, 512);

        let input_node = Box::new(VirtualInputNode::new("in1", "Input", "routune-game"));
        let dsp_node = Box::new(DspBypassNode::new("dsp1", "DSP"));
        let out_node = Box::new(PhysicalOutputNode::new("out1", "Out", "Speakers"));

        controller.add_node(input_node).expect("Add in1 failed");
        controller.add_node(dsp_node).expect("Add dsp1 failed");
        controller.add_node(out_node).expect("Add out1 failed");

        controller.connect("in1", "dsp1").expect("Connect 1 failed");
        controller.connect("dsp1", "out1").expect("Connect 2 failed");

        let ctx = ProcessContext::default();
        let mut final_buf = vec![0.0f32; 16];

        // Drain pending creation commands
        engine.drain_commands();

        // Feed test data
        engine.feed_node_input("in1", &[0.5f32; 16]);
        engine.process_quantum(&mut final_buf, &ctx);

        assert!((final_buf[0] - 0.5).abs() < 1e-4);

        // Remove node dynamically while stream is running
        controller.remove_node("dsp1").expect("Remove dsp1 failed");
        engine.process_quantum(&mut final_buf, &ctx);

        // Verify GC collects decommissioned node off RT thread
        let collected = controller.collect_garbage();
        assert_eq!(collected, 1);
    }
}
