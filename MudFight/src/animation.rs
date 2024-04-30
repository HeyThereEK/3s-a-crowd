use frenderer::sprites::SheetRegion;

#[allow(unused)]
pub enum RepeatMode {
    OneShot,
    Loop,
    PingPong,
}
impl RepeatMode {
    fn map_t(&self, duration: f32, t: f32) -> f32 {
        match self {
            RepeatMode::OneShot => t,
            RepeatMode::Loop => t % duration,
            RepeatMode::PingPong => {
                let quot = t.div_euclid(duration);
                let rem = t.rem_euclid(duration);
                if (quot as u32) % 2 == 0 {
                    rem
                } else {
                    duration - rem
                }
            }
        }
    }
}
pub struct Animation {
    frames: Vec<SheetRegion>,
    timings: Vec<f32>,
    repeat_mode: RepeatMode,
}
#[allow(dead_code)]
impl Animation {
    pub fn with_frame(frame: SheetRegion) -> Self {
        Self::with_frames(&[frame], 1.0)
    }
    pub fn with_frames<'a>(
        frames: impl IntoIterator<Item = &'a SheetRegion>,
        frame_t: f32,
    ) -> Self {
        Self::with_frames_timings(frames, std::iter::repeat(frame_t))
    }
    pub fn with_frames_timings<'a>(
        frames: impl IntoIterator<Item = &'a SheetRegion>,
        timings: impl IntoIterator<Item = f32>,
    ) -> Self {
        let frames: Vec<_> = frames.into_iter().copied().collect();
        let timings: Vec<_> = timings.into_iter().take(frames.len()).collect();
        Self {
            timings,
            frames,
            repeat_mode: RepeatMode::OneShot,
        }
    }
    pub fn looped(self) -> Self {
        Self {
            repeat_mode: RepeatMode::Loop,
            ..self
        }
    }
    pub fn pingpong(self) -> Self {
        Self {
            repeat_mode: RepeatMode::PingPong,
            ..self
        }
    }
    pub fn flip_horizontal(self) -> Self {
        Self {
            frames: self
                .frames
                .into_iter()
                .map(|f| f.flip_horizontal())
                .collect(),
            ..self
        }
    }
    pub fn duration(&self) -> f32 {
        self.timings.iter().sum()
    }
    pub fn sample(&self, t: f32) -> Option<SheetRegion> {
        // convert t into our local time
        let t = self.repeat_mode.map_t(self.duration(), t);
        // find the first frame timing *after* t, and use the one before that one
        let mut net_t = 0.0;
        for (dur, frame) in self.timings.iter().zip(self.frames.iter()) {
            if net_t <= t && t < net_t + dur {
                return Some(*frame);
            }
            net_t += dur;
        }
        None
    }
}
