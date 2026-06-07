use crate::track::audio_track::AudioTrack;

impl AudioTrack {
    // --- LOCAL BUFFER ---

    pub(super) fn init_local_buffers(&mut self) {
        self.local_buffer = vec![0.0; self.audio_ctx.buffer_size * self.audio_ctx.channels];
    }
}
