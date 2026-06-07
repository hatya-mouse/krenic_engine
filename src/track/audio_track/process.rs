use crate::{
    data_types::{AudioContext, Beats},
    graph::{Graph, error::GraphError},
    mixer::TempoMap,
    track::{
        RegionID, Track,
        audio_track::{AudioTrack, tempo_strech::tempo_strech},
    },
};

impl AudioTrack {
    // --- LOCAL BUFFER ---

    fn init_local_buffers(&mut self) {
        self.local_buffer = vec![0.0; self.audio_ctx.buffer_size];
    }
}

impl Track for AudioTrack {
    // --- CLONING ---

    fn clone_box(&self) -> Box<dyn Track> {
        Box::new(self.clone())
    }

    // --- GRAPH GETTING ---

    fn get_graph(&self) -> &Graph {
        &self.graph
    }

    fn get_graph_mut(&mut self) -> &mut Graph {
        &mut self.graph
    }

    // --- GRAPH UPDATING ---

    fn set_graph(&mut self, graph: Graph) {
        self.graph = graph;
    }

    // --- AUDIO CONTEXT UPDARING ---

    fn set_audio_ctx(&mut self, audio_ctx: &AudioContext) {
        self.audio_ctx = audio_ctx.clone();
        self.graph.set_audio_ctx(audio_ctx);
    }

    // --- REGION MODIFICATION ---

    fn move_region(&mut self, region_id: &RegionID, new_start: Beats) {
        if let Some(region) = self.regions.get_mut(region_id) {
            region.start = new_start;
        }
    }

    fn set_region_duration(&mut self, region_id: &RegionID, new_duration: Beats) {
        if let Some(region) = self.regions.get_mut(region_id) {
            region.duration = new_duration;
        }
    }

    fn remove_region(&mut self, region_id: &RegionID) {
        self.regions.remove(region_id);
    }

    // --- SEEKING ---

    fn seek(&mut self, _playhead: usize) {}

    // --- TRACK PROCESSING ---

    fn prepare(
        &mut self,
        _start: usize,
        duration: usize,
        tempo_map: &TempoMap,
    ) -> Result<(), GraphError> {
        // Calculate the total sample number
        // Ceil to a multiple of the buffer size
        let total_frames =
            duration.div_ceil(self.audio_ctx.buffer_size) * self.audio_ctx.buffer_size;
        // Initialize the processed vector with zeros
        self.pre_processed = vec![0.0; total_frames * self.audio_ctx.channels];

        // Resample the each regions
        for region in self.regions.values() {
            let resampled = tempo_strech(
                region,
                self.audio_ctx.sample_rate,
                self.audio_ctx.channels,
                tempo_map,
            );

            // Calculate the start sample index of the buffer
            let region_start_index = tempo_map.beats_to_samples(region.start);

            // Add the resampled samples
            let available = self.pre_processed.len().saturating_sub(region_start_index);
            let copy_len = resampled.len().min(available);
            for (i, sample) in resampled[..copy_len].iter().enumerate() {
                self.pre_processed[region_start_index + i] += sample;
            }
        }

        // Initialize the local buffers
        self.init_local_buffers();

        // Then prepare the graph
        self.graph.prepare()
    }

    fn process_to_local_buffer(&mut self, is_playing: bool, playhead: usize) {
        if is_playing {
            let buffer_size = self.audio_ctx.buffer_size * self.audio_ctx.channels;
            let buffer_end = playhead + buffer_size;

            // Create a vector for input buffer
            let mut input_vec: Vec<f32>;

            let input_ptr = if buffer_end <= self.pre_processed.len() {
                // Get a pointer to the input buffer
                self.pre_processed[playhead..buffer_end].as_ptr() as *const u8
            } else {
                // If the audio data for the buffer is partially unavailable fill the rest with zero
                let available = self.pre_processed.len().saturating_sub(playhead);
                input_vec = vec![0f32; buffer_size];
                if available > 0 {
                    input_vec[..available]
                        .copy_from_slice(&self.pre_processed[playhead..playhead + available]);
                }
                input_vec.as_ptr() as *const u8
            };

            // Process the graph
            self.graph
                .process(&[input_ptr], &[self.local_buffer.as_mut_ptr() as *mut u8]);
        }
    }

    fn get_local_buffer(&self) -> &[f32] {
        &self.local_buffer
    }

    // --- ANY CASTING ---

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
