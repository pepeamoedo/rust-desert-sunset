#[cfg(target_arch = "wasm32")]

#[cfg(target_arch = "wasm32")]
use web_sys::{AudioContext, GainNode, BiquadFilterNode};
use std::sync::Arc;
use crate::EnvironmentState;

pub struct WindAudioController {
    #[cfg(target_arch = "wasm32")]
    ctx: AudioContext,
    #[cfg(target_arch = "wasm32")]
    gain: GainNode,
    #[cfg(target_arch = "wasm32")]
    filter: BiquadFilterNode,
    env_state: Arc<EnvironmentState>,
    time: f32,
}

impl WindAudioController {
    pub fn new(env_state: Arc<EnvironmentState>) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            let ctx = AudioContext::new().unwrap();
            
            // Create white noise buffer
            let sample_rate = ctx.sample_rate() as u32;
            let buffer_size = sample_rate * 2; // 2 seconds of noise
            let buffer = ctx.create_buffer(1, buffer_size, sample_rate as f32).unwrap();
            
            let mut data = vec![0.0f32; buffer_size as usize];
            for i in 0..buffer_size as usize {
                data[i] = rand::random::<f32>() * 2.0 - 1.0;
            }
            
            buffer.copy_to_channel(&mut data, 0).unwrap();
            
            // Create source node
            let source = ctx.create_buffer_source().unwrap();
            source.set_buffer(Some(&buffer));
            source.set_loop(true);
            
            // Create lowpass filter
            let filter = ctx.create_biquad_filter().unwrap();
            filter.set_type(web_sys::BiquadFilterType::Lowpass);
            filter.frequency().set_value(100.0);
            
            // Create gain node for volume control
            let gain = ctx.create_gain().unwrap();
            gain.gain().set_value(0.5);
            
            // Connect nodes: Source -> Filter -> Gain -> Destination
            source.connect_with_audio_node(&filter).unwrap();
            filter.connect_with_audio_node(&gain).unwrap();
            gain.connect_with_audio_node(&ctx.destination()).unwrap();
            
            // Start playing
            source.start().unwrap();

            Self {
                ctx,
                gain,
                filter,
                env_state,
                time: 0.0,
            }
        }
        
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {
                env_state,
                time: 0.0,
            }
        }
    }

    pub fn update(&mut self) {
        let wind_speed = self.env_state.get_wind_speed();
        self.time += 0.016 * wind_speed; // approximate 60fps delta
        
        // Modulate with slow sine wave (wind gusts)
        let gust = (self.time * 0.4).sin() * 0.5 + 0.5; // 0.0 to 1.0
        
        // Base frequency 50Hz, scales up to 400Hz depending on wind speed and gust
        let freq = 50.0 + (350.0 * gust * wind_speed);
        
        // Volume depends on wind speed and gust
        let volume = (0.2 + gust * 0.4) * wind_speed;
        
        #[cfg(target_arch = "wasm32")]
        {
            self.filter.frequency().set_value(freq);
            self.gain.gain().set_value(volume);
            
            // Resume context if user interacted but it was suspended
            if self.ctx.state() == web_sys::AudioContextState::Suspended {
                let _ = self.ctx.resume();
            }
        }
    }
}
