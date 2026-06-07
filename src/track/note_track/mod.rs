mod note;
mod note_region;
mod process;
mod voice_event;

pub use note::{Note, NoteID};
pub use note_region::NoteRegion;

use crate::{
    data_types::{AudioContext, Voice},
    graph::Graph,
    node::builtin::{AudioOutputNode, NoteInputNode},
    track::RegionID,
};
use std::collections::{HashMap, VecDeque};
use voice_event::VoiceEvent;

#[derive(Default, Clone)]
pub struct NoteTrack {
    // --- GRAPH ---
    graph: Graph,

    // --- NOTE DATA ---
    regions: HashMap<RegionID, NoteRegion>,

    // --- VOICE MANAGEMENT ---
    events: Vec<VoiceEvent>,
    event_cursor: usize,
    active_voices: VecDeque<(usize, f32)>,
    free_voices: Vec<usize>,
    last_voices: Vec<Voice>,
    voice_buffer: Vec<Voice>,
    // Live MIDI voices: MIDI note number -> voice index
    live_voices: HashMap<u8, usize>,

    // --- LOCAL OUTPUT BUFFER ---
    local_buffer: Vec<f32>,

    // --- AUDIO CONTEXT ---
    audio_ctx: AudioContext,

    // --- MISC ---
    next_region_id: usize,
}

impl NoteTrack {
    pub fn new(audio_ctx: AudioContext) -> Self {
        // Create a graph with the input and output nodes
        let input_node = NoteInputNode::default();
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

    pub fn get_region(&self, id: &RegionID) -> Option<&NoteRegion> {
        self.regions.get(id)
    }

    pub fn get_region_mut(&mut self, id: &RegionID) -> Option<&mut NoteRegion> {
        self.regions.get_mut(id)
    }

    pub fn get_all_regions(&self) -> &HashMap<RegionID, NoteRegion> {
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

    pub fn add_region(&mut self, region: NoteRegion) -> RegionID {
        let id = self.generate_region_id();
        self.regions.insert(id, region);
        id
    }

    pub fn set_regions(&mut self, regions: HashMap<RegionID, NoteRegion>) {
        self.regions = regions;
    }
}
