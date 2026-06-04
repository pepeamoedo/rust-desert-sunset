#[cfg(not(target_arch = "wasm32"))]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use crate::EnvironmentState;

#[cfg(not(target_arch = "wasm32"))]
struct FastRng { state: u64 }
#[cfg(not(target_arch = "wasm32"))]
impl FastRng {
    fn new() -> Self { Self { state: 0x2545F4914F6CDD1D } }
    fn next_f32(&mut self) -> f32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        let rand_val = (self.state >> 40) as u32;
        (rand_val as f32) / 16777216.0 * 2.0 - 1.0
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct Biquad {
    b0: f32, b1: f32, b2: f32,
    a1: f32, a2: f32,
    x1: f32, x2: f32,
    y1: f32, y2: f32,
}

#[cfg(not(target_arch = "wasm32"))]
impl Biquad {
    fn new() -> Self {
        Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 }
    }
    
    fn set_bandpass(&mut self, f0: f32, q: f32, fs: f32) {
        let w0 = 2.0 * std::f32::consts::PI * f0 / fs;
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        self.b0 = alpha / a0;
        self.b1 = 0.0;
        self.b2 = -alpha / a0;
        self.a1 = (-2.0 * w0.cos()) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }
    
    fn set_lowpass(&mut self, f0: f32, q_db: f32, fs: f32) {
        let w0 = 2.0 * std::f32::consts::PI * f0 / fs;
        let q_linear = 10.0f32.powf(q_db / 20.0);
        let alpha = w0.sin() / (2.0 * q_linear);
        let a0 = 1.0 + alpha;
        self.b0 = ((1.0 - w0.cos()) / 2.0) / a0;
        self.b1 = (1.0 - w0.cos()) / a0;
        self.b2 = ((1.0 - w0.cos()) / 2.0) / a0;
        self.a1 = (-2.0 * w0.cos()) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }
    
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
              - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1; self.x1 = x;
        self.y2 = self.y1; self.y1 = y;
        
        // Prevent denormals/NaNs
        if y.is_nan() || y.is_infinite() || y.abs() < 1e-10 {
            return 0.0;
        }
        y
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
    
    let mut filter_bp = Biquad::new();
    let mut filter_lp = Biquad::new();
    let mut lfo = Lfo::new();
    
    // Allocate 2 seconds of noise buffer, EXACTLY like Web Audio API!
    let buffer_size = (sample_rate * 2.0) as usize;
    let mut noise_buffer = vec![0.0f32; buffer_size];
    let mut rng = FastRng::new();
    for i in 0..buffer_size {
        noise_buffer[i] = rng.next_f32();
    }
    let mut noise_idx = 0;
    
    // Lowpass is static
    filter_lp.set_lowpass(90.0, 1.0, sample_rate);
    
    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);
    
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let wind_speed = env_state.get_wind_speed();
            let freq = 100.0 + (300.0 * wind_speed);
            let volume = 0.5 * wind_speed;
            let lfo_rate = 0.2 + (0.8 * wind_speed);
            
            for frame in data.chunks_mut(channels) {
                // Exact Web Audio noise looping behavior
                let noise = noise_buffer[noise_idx];
                noise_idx = (noise_idx + 1) % buffer_size;
                
                let lfo_val = lfo.process(lfo_rate, sample_rate);
                
                // Web Audio clamps frequency to nominal range [10, Nyquist] for Biquads
                let modulated_freq = (freq + lfo_val * 400.0).clamp(10.0, sample_rate / 2.0 - 1.0);
                
                // Update bandpass coefficients
                filter_bp.set_bandpass(modulated_freq, 5.0, sample_rate);
                
                // WebAudio routes the exact same noise node to both filters!
                let bp_out = filter_bp.process(noise);
                let lp_out = filter_lp.process(noise);
                
                // Mix exactly like WebAudio Gain node
                let mut out = (bp_out + lp_out) * volume;
                
                // WebAudio has soft clipping at the destination
                out = out.tanh();
                
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
