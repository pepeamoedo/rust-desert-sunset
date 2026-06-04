#[cfg(not(target_arch = "wasm32"))]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use crate::EnvironmentState;

#[cfg(not(target_arch = "wasm32"))]
struct FastRng { state: u32 }

#[cfg(not(target_arch = "wasm32"))]
impl FastRng {
    fn new() -> Self { Self { state: 42 } }
    fn next_f32(&mut self) -> f32 {
        self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.state as f32) / (u32::MAX as f32) * 2.0 - 1.0
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct Svf {
    lp: f32,
    bp: f32,
}

#[cfg(not(target_arch = "wasm32"))]
impl Svf {
    fn new() -> Self { Self { lp: 0.0, bp: 0.0 } }
    
    // Process 2x oversampled Chamberlin SVF for stability
    fn process(&mut self, input: f32, f0: f32, q: f32, fs: f32) -> (f32, f32) {
        let f = 2.0 * (std::f32::consts::PI * f0 / (fs * 2.0)).sin();
        let damp = 1.0 / q;
        
        // Iteration 1
        let mut hp = input - self.lp - damp * self.bp;
        self.bp += f * hp;
        self.lp += f * self.bp;
        
        // Iteration 2
        hp = input - self.lp - damp * self.bp;
        self.bp += f * hp;
        self.lp += f * self.bp;
        
        // Return 0dB peak normalized bandpass, and lowpass
        (self.bp * damp, self.lp)
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct Lfo { phase: f32 }

#[cfg(not(target_arch = "wasm32"))]
impl Lfo {
    fn new() -> Self { Self { phase: 0.0 } }
    fn process(&mut self, freq: f32, fs: f32) -> f32 {
        self.phase += 2.0 * std::f32::consts::PI * freq / fs;
        if self.phase > 2.0 * std::f32::consts::PI { self.phase -= 2.0 * std::f32::consts::PI; }
        self.phase.sin()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_audio_stream(env_state: Arc<EnvironmentState>) -> Option<cpal::Stream> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let config = device.default_output_config().ok()?;
    
    match config.sample_format() {
        cpal::SampleFormat::F32 => build_stream::<f32>(&device, &config.into(), env_state),
        cpal::SampleFormat::I16 => build_stream::<i16>(&device, &config.into(), env_state),
        cpal::SampleFormat::U16 => build_stream::<u16>(&device, &config.into(), env_state),
        _ => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn build_stream<T>(device: &cpal::Device, config: &cpal::StreamConfig, env_state: Arc<EnvironmentState>) -> Option<cpal::Stream>
where
    T: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>,
{
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;
    
    let mut filter_bp = Svf::new();
    let mut filter_lp = Svf::new();
    let mut lfo = Lfo::new();
    let mut rng = FastRng::new();
    
    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);
    
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let wind_speed = env_state.get_wind_speed();
            let freq = 100.0 + (300.0 * wind_speed);
            let volume = 0.5 * wind_speed;
            let lfo_rate = 0.2 + (0.8 * wind_speed);
            
            for frame in data.chunks_mut(channels) {
                let noise = rng.next_f32();
                
                let lfo_val = lfo.process(lfo_rate, sample_rate);
                let modulated_freq = (freq + lfo_val * 400.0).clamp(20.0, sample_rate / 2.0 - 1.0);
                
                // Process Bandpass (modulating)
                let (bp_out, _) = filter_bp.process(noise, modulated_freq, 5.0, sample_rate);
                
                // Process Lowpass (static)
                let (_, lp_out) = filter_lp.process(noise, 90.0, 1.0, sample_rate);
                
                let out = (bp_out + lp_out) * volume;
                let sample: T = T::from_sample(out);
                
                for output in frame.iter_mut() {
                    *output = sample;
                }
            }
        },
        err_fn,
        None
    ).ok()?;
    
    stream.play().ok()?;
    Some(stream)
}
