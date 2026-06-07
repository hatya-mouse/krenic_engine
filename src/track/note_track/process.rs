use crate::{
    data_types::{AudioContext, Beats, MidiEvent, Voice},
    graph::{Graph, error::GraphError},
    mixer::TempoMap,
    track::{
        RegionID, Track,
        note_track::{NoteTrack, VoiceEvent},
    },
};

impl NoteTrack {
    // --- VOICE GETTING ---

    /// Returns the vacant voice index, or returns the index of the oldest voice.
    fn find_or_steal_voice(&mut self, new_freq: f32) -> usize {
        let new_voice_index = self
            .free_voices
            .pop()
            .or_else(|| self.active_voices.pop_front().map(|v| v.0))
            .unwrap_or_default();
        self.active_voices.push_back((new_voice_index, new_freq));
        new_voice_index
    }

    // --- REALTIME MIDI ---

    /// Receives live MIDI events and updates the voice state.
    /// Must be called before process() so that changes take effect from sample 0 of the buffer.
    pub fn pass_midi(&mut self, events: &[MidiEvent]) {
        for event in events {
            match event {
                MidiEvent::NoteOn { pitch, velocity } => {
                    // Allocate from the shared pool, stealing the oldest sequenced voice if full
                    let voice_idx = self
                        .free_voices
                        .pop()
                        .or_else(|| self.active_voices.pop_front().map(|(vi, _)| vi))
                        .unwrap_or(0);
                    self.live_voices.insert(*pitch, voice_idx);
                    if let Some(v) = self.last_voices.get_mut(voice_idx) {
                        *v = Voice::new(*pitch as f32, *velocity as f32 / 127.0, 0.0, true);
                    }
                }
                MidiEvent::NoteOff { pitch } => {
                    if let Some(voice_idx) = self.live_voices.remove(pitch) {
                        self.free_voices.push(voice_idx);
                        if let Some(v) = self.last_voices.get_mut(voice_idx) {
                            v.is_active = false;
                            v.age = 0.0;
                        }
                    }
                }
            }
        }
    }
}

impl Track for NoteTrack {
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

    // --- AUDIO CONTEXT UPDARING ---

    fn set_audio_ctx(&mut self, audio_ctx: &AudioContext) {
        self.audio_ctx = audio_ctx.clone();
        self.graph.set_audio_ctx(audio_ctx);
    }

    // --- SEEKING ---

    fn seek(&mut self, playhead: usize) {
        // Clear all voices before seeking
        self.active_voices.clear();
        self.live_voices.clear();
        self.free_voices = (0..self.audio_ctx.max_voices).collect();
        self.last_voices = vec![Voice::default(); self.audio_ctx.max_voices];
        // Recalculate the event cursor
        self.event_cursor = self.events.partition_point(|e| e.sample_index < playhead);
    }

    // --- TRACK PROCESSING ---

    fn prepare(
        &mut self,
        _start: usize,
        _duration: usize,
        tempo_map: &TempoMap,
    ) -> Result<(), GraphError> {
        // Clear the old events
        self.events.clear();

        // Retrieve the notes from the regions in the track
        for region in self.regions.values() {
            let region_end = region.start + region.duration;

            // Calculate the start sample of the region
            for note in region.notes.values() {
                let note_end = note.start + note.duration;

                // Calculate the start and end sample of the note in the entire track
                let absolute_note_start = region.start + note.start;
                let absolute_note_end = region.start + note_end;

                // Skip the note if it is outside the region
                // Skip if absolute_note_start equals region_end to prevent NOTE OFF event
                // from occuring at the same time as the NOTE ON
                if absolute_note_start >= region_end || absolute_note_end < region.start {
                    continue;
                }

                // Clamp the start and the end beats by the region start and the end
                let clamped_note_start = absolute_note_start.max(region.start);
                let clamped_note_end = absolute_note_end.min(region_end);

                // Convert the start and end beats to sampels
                let absolute_start_sample = tempo_map.beats_to_samples(clamped_note_start);
                let absolute_end_sample = tempo_map.beats_to_samples(clamped_note_end);

                // Add the note start and end event to the events
                self.events.push(VoiceEvent::new(
                    absolute_start_sample,
                    note.pitch,
                    note.velocity,
                    true,
                ));
                self.events.push(VoiceEvent::new(
                    absolute_end_sample,
                    note.pitch,
                    note.velocity,
                    false,
                ));
            }
        }

        // Sort the events
        self.events.sort_unstable_by_key(|e| e.sample_index);

        // Initialize the voice buffer
        self.voice_buffer =
            vec![Voice::default(); self.audio_ctx.buffer_size * self.audio_ctx.max_voices];

        // Initialize the voices
        self.active_voices.clear();
        self.free_voices = (0..self.audio_ctx.max_voices).collect();
        self.last_voices = vec![Voice::default(); self.audio_ctx.max_voices];

        // Prepare the graph
        self.graph.prepare()
    }

    fn process_to_local_buffer(&mut self, is_playing: bool, playhead: usize) {
        // Convert the playhead beats to samples
        let buffer_end = playhead + self.audio_ctx.buffer_size;
        let max_voices = self.audio_ctx.max_voices;

        // Seek the event cursor
        if self
            .events
            .get(self.event_cursor)
            .is_some_and(|e| e.sample_index > playhead)
            || (self.event_cursor > 0 && self.events[self.event_cursor - 1].sample_index > playhead)
        {
            self.event_cursor = self.events.partition_point(|e| e.sample_index < playhead);
        }

        for sample in playhead..buffer_end {
            // Calculate the local sample in the buffer chunk
            let local_sample = sample - playhead;
            // Calculate the index of the first current voice
            let current = local_sample * max_voices;

            // If the current sample is the first sample in the buffer,
            // Copy from the last voices
            if local_sample == 0 && !self.last_voices.is_empty() {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        self.last_voices.as_ptr(),
                        self.voice_buffer.as_mut_ptr(),
                        max_voices,
                    );
                }
            }

            // If the current sample is not the first sample in the buffer,
            // copy the previous voices to the current index
            if local_sample > 0 {
                let previous = (local_sample - 1) * max_voices;
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        self.voice_buffer[previous..].as_ptr(),
                        self.voice_buffer[current..].as_mut_ptr(),
                        max_voices,
                    );
                }
            }

            // Increment age for live midi voices
            for &index in self.live_voices.values() {
                self.voice_buffer[current + index].age += 1.0 / self.audio_ctx.sample_rate as f32;
            }

            // Process the sequenced voices when playing
            if is_playing {
                // Increment age for sequenced voices
                for (index, _) in self.active_voices.iter() {
                    self.voice_buffer[current + index].age +=
                        1.0 / self.audio_ctx.sample_rate as f32;
                }

                // Consume the events in this sample
                while let Some(event) = self.events.get(self.event_cursor) {
                    // Break if the event is in future
                    if event.sample_index > sample {
                        break;
                    }
                    // If the event is the past event, skip the event
                    if event.sample_index < sample {
                        self.event_cursor += 1;
                        continue;
                    }

                    // Copy the frequency and velocity to avoid reference issues
                    let frequency = event.frequency;
                    let velocity = event.velocity;

                    if event.is_note_on {
                        // Start playing the note from the sample
                        let voice_index = self.find_or_steal_voice(frequency);
                        // Set the new voice to the voice buffer
                        self.voice_buffer[current + voice_index] =
                            Voice::new(frequency, velocity, 0.0, true);
                    } else {
                        // Remove the active voice whose frequency matches the event frequency
                        if let Some(remove_index) = self
                            .active_voices
                            .iter()
                            .position(|(_, freq)| *freq == event.frequency)
                        {
                            // Remove the index from the active_voices and get the voice index
                            let (voice_index, _) = self.active_voices.remove(remove_index).unwrap();
                            // Mark the voice index as free
                            self.free_voices.push(voice_index);
                            self.voice_buffer[current + voice_index].is_active = false;
                            self.voice_buffer[current + voice_index].age = 0.0;
                        }
                    }

                    // Increment the event cursor
                    self.event_cursor += 1;
                }
            }
        }

        // Copy the last voices
        let last = (self.audio_ctx.buffer_size - 1) * max_voices;
        self.last_voices
            .clone_from_slice(&self.voice_buffer[last..last + max_voices]);

        // Get a pointer to the voice buffer
        let input_ptr = self.voice_buffer.as_ptr() as *const u8;
        // Process the graph
        self.graph
            .process(&[input_ptr], &[self.local_buffer.as_mut_ptr() as *mut u8]);
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
