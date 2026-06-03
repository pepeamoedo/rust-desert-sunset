#[cfg(target_arch = "wasm32")]

#[cfg(target_arch = "wasm32")]
use web_sys::{AudioContext, GainNode, BiquadFilterNode, OscillatorNode};
use std::sync::Arc;
use crate::EnvironmentState;

pub struct WindAudioController {
    #[cfg(target_arch = "wasm32")]
    ctx: AudioContext,
    #[cfg(target_arch = "wasm32")]
    gain: GainNode,
    #[cfg(target_arch = "wasm32")]
    filter: BiquadFilterNode,
    #[cfg(target_arch = "wasm32")]
    lfo: OscillatorNode,
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
            
            // Create bandpass filter
            let filter = ctx.create_biquad_filter().unwrap();
            filter.set_type(web_sys::BiquadFilterType::Bandpass);
            filter.frequency().set_value(200.0);
            filter.q().set_value(5.0); // High Q factor for howling wind sound
            
            // Create LFO (Low-Frequency Oscillator) to modulate the filter frequency
            let lfo = ctx.create_oscillator().unwrap();
            lfo.set_type(web_sys::OscillatorType::Sine);
            lfo.frequency().set_value(0.5); // Very slow frequency (0.5 Hz)
            
            // Create a gain node to scale the LFO output before it hits the filter frequency
            let lfo_gain = ctx.create_gain().unwrap();
            lfo_gain.gain().set_value(400.0); // Modulates the frequency by +/- 400Hz
            
            // Connect LFO -> lfo_gain -> filter.frequency (AudioParam)
            lfo.connect_with_audio_node(&lfo_gain).unwrap();
            lfo_gain.connect_with_audio_param(&filter.frequency()).unwrap();
            
            // Create main gain node for volume control
            let gain = ctx.create_gain().unwrap();
            gain.gain().set_value(0.5);
            
            // Connect audio graph: Source -> Filter -> Gain -> Destination
            source.connect_with_audio_node(&filter).unwrap();
            filter.connect_with_audio_node(&gain).unwrap();
            gain.connect_with_audio_node(&ctx.destination()).unwrap();
            
            // Start playing
            source.start().unwrap();
            lfo.start().unwrap();

            Self {
                ctx,
                gain,
                filter,
                lfo,
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
        
        // Base frequency moves up with wind speed
        let freq = 100.0 + (300.0 * wind_speed);
        
        // Volume depends on wind speed
        let volume = 0.5 * wind_speed;
        
        // LFO rate depends on wind speed (faster wind = faster gusts)
        let lfo_rate = 0.2 + (0.8 * wind_speed);
        
        #[cfg(target_arch = "wasm32")]
        {
            // Base filter frequency
            self.filter.frequency().set_value(freq);
            
            // Update LFO rate
            self.lfo.frequency().set_value(lfo_rate);
            
            // Update volume
            self.gain.gain().set_value(volume);
            
            // Resume context if user interacted but it was suspended
            if self.ctx.state() == web_sys::AudioContextState::Suspended {
                let _ = self.ctx.resume();
            }
        }
    }
}
