use std::ptr::copy_nonoverlapping;

use crate::{
    data_types::Voice,
    mixer::TempoMap,
    track::note_track::{NoteTrack, VoiceEvent},
};

impl NoteTrack {
    // --- VOICE GETTING ---

    /// Returns the vacant voice index, or returns the index of the oldest voice.
    pub(super) fn find_or_steal_voice(&mut self, new_freq: f32) -> usize {
        let new_voice_index = self
            .free_voices
            .pop()
            .or_else(|| self.active_voices.pop_front().map(|v| v.0))
            .unwrap_or_default();
        self.active_voices.push_back((new_voice_index, new_freq));
        new_voice_index
    }

    // --- PREPARATION ---

    /// Retrieves the notes from the regions and converts them to events.
    pub(super) fn retrieve_and_register_notes(&mut self, tempo_map: &TempoMap) {
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
    }

    /// Initializes the voices.
    pub(super) fn init_voices(&mut self) {
        // Initialize the voice buffer
        self.voice_buffer =
            vec![Voice::default(); self.audio_ctx.buffer_size * self.audio_ctx.max_voices];
        // Clear the active voices and the free voices
        self.active_voices.clear();
        self.free_voices = (0..self.audio_ctx.max_voices).collect();
        self.last_voices = vec![Voice::default(); self.audio_ctx.max_voices];
    }

    /// Initializes the local buffer based on the buffer size.
    pub(super) fn init_local_buffer(&mut self) {
        self.local_buffer = vec![0.0; self.audio_ctx.buffer_size];
    }

    // --- PROCESS ---

    /// Seeks the event cursor to the current playhead position.
    pub(super) fn seek_event_cursor(&mut self, playhead: usize) {
        if self
            .events
            .get(self.event_cursor)
            .is_some_and(|e| e.sample_index > playhead)
            || (self.event_cursor > 0 && self.events[self.event_cursor - 1].sample_index > playhead)
        {
            self.event_cursor = self.events.partition_point(|e| e.sample_index < playhead);
        }
    }

    /// Propagates the voice data from the previous sample to the current sample.
    pub(super) fn propagate_voices(
        &mut self,
        local_sample: usize,
        max_voices: usize,
        current: usize,
    ) {
        // If the current sample is the first sample in the buffer,
        // Copy from the last voices
        if local_sample == 0 && !self.last_voices.is_empty() {
            unsafe {
                copy_nonoverlapping(
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
                copy_nonoverlapping(
                    self.voice_buffer[previous..].as_ptr(),
                    self.voice_buffer[current..].as_mut_ptr(),
                    max_voices,
                );
            }
        }
    }

    /// Updates the ages for each voices.
    pub(super) fn increment_ages(&mut self, current: usize) {
        for &index in self.live_voices.values() {
            self.voice_buffer[current + index].age += 1.0 / self.audio_ctx.sample_rate as f32;
        }
    }

    /// Consumes the events at the current sample and updates the voice buffer accordingly.
    pub(super) fn consume_events_at_sample(&mut self, sample: usize, current: usize) {
        // Increment age for sequenced voices
        for (index, _) in self.active_voices.iter() {
            self.voice_buffer[current + index].age += 1.0 / self.audio_ctx.sample_rate as f32;
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
