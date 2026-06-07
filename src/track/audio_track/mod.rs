mod audio_region;
mod process;
mod resampler;
mod tempo_strech;

pub use audio_region::AudioRegion;

use crate::{
    data_types::AudioContext,
    graph::Graph,
    node::builtin::{AudioInputNode, AudioOutputNode},
    track::RegionID,
};
use std::collections::HashMap;

#[derive(Default, Clone)]
pub struct AudioTrack {
    // --- GRAPH ---
    graph: Graph,

    // --- RAW AUDIO DATA ---
    regions: HashMap<RegionID, AudioRegion>,
    pre_processed: Vec<f32>,

    // --- LOCAL BUFFER ---
    local_buffer: Vec<f32>,

    // --- AUDIO CONTEXT ---
    audio_ctx: AudioContext,

    // --- MISC ---
    next_region_id: usize,
}

impl AudioTrack {
    pub fn new(audio_ctx: AudioContext) -> Self {
        // Create a graph with the input and output nodes
        let input_node = AudioInputNode::default();
        let output_node = AudioOutputNode::default();
        let graph = Graph::new(
            Box::new(input_node),
            Box::new(output_node),
            audio_ctx.clone(),
        );

        Self {
            graph,
            audio_ctx,
            ..Default::default()
        }
    }

    // --- REGION GETTING ---

    pub fn get_region(&self, id: &RegionID) -> Option<&AudioRegion> {
        self.regions.get(id)
    }

    pub fn get_region_mut(&mut self, id: &RegionID) -> Option<&mut AudioRegion> {
        self.regions.get_mut(id)
    }

    pub fn get_all_regions(&self) -> &HashMap<RegionID, AudioRegion> {
        &self.regions
    }

    // --- REGION ADDITION ---

    pub fn set_next_region_id(&mut self, next_id: usize) {
        self.next_region_id = next_id;
    }

    fn generate_region_id(&mut self) -> RegionID {
        let id = RegionID(self.next_region_id);
        self.next_region_id += 1;
        id
    }

    pub fn add_region(&mut self, region: AudioRegion) -> RegionID {
        let id = self.generate_region_id();
        self.regions.insert(id, region);
        id
    }

    pub fn set_regions(&mut self, regions: HashMap<RegionID, AudioRegion>) {
        self.regions = regions;
    }
}
