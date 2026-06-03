use rodio::source::Source;
use std::time::Duration;
use rand::Rng;
use std::sync::Arc;
use crate::EnvironmentState;

pub struct WindSource {
    time: f32,
    sample_rate: u32,
    prev_val: f32,
    env_state: Arc<EnvironmentState>,
}

impl WindSource {
    pub fn new(sample_rate: u32, env_state: Arc<EnvironmentState>) -> Self {
        Self {
            time: 0.0,
            sample_rate,
            prev_val: 0.0,
            env_state,
        }
    }
}

impl Iterator for WindSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let wind_speed = self.env_state.get_wind_speed();
        
        self.time += (1.0 / self.sample_rate as f32) * wind_speed;
        
        // Generate white noise using thread-local fast rng
        let noise: f32 = rand::thread_rng().gen_range(-1.0..1.0);
        
        // Modulate cutoff frequency with a slow sine wave (wind gusts)
        let gust = (self.time * 0.4).sin() * 0.5 + 0.5; // 0.0 to 1.0
        let cutoff = 0.01 + gust * 0.03; // Very low cutoff for bassy rumble
        
        // Simple One-Pole Low-Pass Filter
        self.prev_val = self.prev_val + cutoff * (noise - self.prev_val);
        
        // Scale volume based on gust and wind speed
        let volume = (0.5 + gust * 0.5) * wind_speed;
        
        Some(self.prev_val * volume * 3.0)
    }
}

impl Source for WindSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        1 // Mono
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None // Infinite
    }
}
