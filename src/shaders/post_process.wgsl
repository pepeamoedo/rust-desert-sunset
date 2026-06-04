struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let x = f32((in_vertex_index & 1u) << 2u);
    let y = f32((in_vertex_index & 2u) << 1u);
    out.clip_position = vec4<f32>(x - 1.0, 1.0 - y, 0.0, 1.0);
    out.uv = vec2<f32>(x * 0.5, y * 0.5);
    return out;
}

@group(0) @binding(0) var t_color: texture_2d<f32>;
@group(0) @binding(1) var s_color: sampler;

struct PostProcessUniforms {
    time: f32,
    resolution: vec2<f32>,
    _padding: f32,
};
@group(0) @binding(2) var<uniform> uniforms: PostProcessUniforms;

// Simple random function based on UV and time
fn random(uv: vec2<f32>, time: f32) -> f32 {
    return fract(sin(dot(uv.xy, vec2<f32>(12.9898, 78.233)) + time) * 43758.5453123);
}

// La textura del fotograma anterior copiada al final del render pass
@group(0) @binding(3) var t_history: texture_2d<f32>; 

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let current_raw = textureSample(t_color, s_color, in.uv).rgb;
    let history_color = textureSample(t_history, s_color, in.uv).rgb;
    
    // --- Light Bloom ---
    var bloom = vec3<f32>(0.0);
    let bloom_threshold = 0.8;
    
    // Simple 5-tap cross blur for bloom on the CURRENT frame
    let texel_size = vec2<f32>(1.0 / uniforms.resolution.x, 1.0 / uniforms.resolution.y);
    var offsets = array<vec2<f32>, 5>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(-1.5, 0.0) * texel_size,
        vec2<f32>(1.5, 0.0) * texel_size,
        vec2<f32>(0.0, -1.5) * texel_size,
        vec2<f32>(0.0, 1.5) * texel_size
    );
    
    for (var i = 0; i < 5; i++) {
        let sample_uv = in.uv + offsets[i];
        let sample_col = textureSample(t_color, s_color, sample_uv).rgb;
        let luminance = dot(sample_col, vec3<f32>(0.299, 0.587, 0.114));
        if (luminance > bloom_threshold) {
            bloom += sample_col * (luminance - bloom_threshold) * 0.2;
        }
    }
    
    var processed_color = current_raw + bloom;
    
    // --- Color Grading (Etalonaje) ---
    // S-Curve Contrast
    let contrast = 1.1;
    processed_color = processed_color - 0.5;
    processed_color = processed_color * contrast + 0.5;
    processed_color = clamp(processed_color, vec3<f32>(0.0), vec3<f32>(1.0));
    
    // Warm Tint (Cinematic Sunset)
    let warm_tint = vec3<f32>(1.05, 0.98, 0.92); 
    processed_color *= warm_tint;
    
    // --- TAA (Acumulación Temporal Exponencial) ---
    // Mezclar el frame actual (ya procesado) con el historial
    let final_color = mix(history_color, processed_color, 0.1);
    
    return vec4<f32>(final_color, 1.0);
}
