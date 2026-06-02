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

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let px = 1.0 / uniforms.resolution;
    var color = vec3<f32>(0.0);
    
    // Spatial Denoising (3x3 Blur)
    // Smooths out the high-frequency stochastic noise introduced by volumetric ray jittering
    color += textureSample(t_color, s_color, in.uv + vec2<f32>(-px.x, -px.y)).rgb * 0.0625;
    color += textureSample(t_color, s_color, in.uv + vec2<f32>( 0.0, -px.y)).rgb * 0.125;
    color += textureSample(t_color, s_color, in.uv + vec2<f32>( px.x, -px.y)).rgb * 0.0625;
    
    color += textureSample(t_color, s_color, in.uv + vec2<f32>(-px.x,  0.0)).rgb * 0.125;
    color += textureSample(t_color, s_color, in.uv + vec2<f32>( 0.0,  0.0)).rgb * 0.25;
    color += textureSample(t_color, s_color, in.uv + vec2<f32>( px.x,  0.0)).rgb * 0.125;
    
    color += textureSample(t_color, s_color, in.uv + vec2<f32>(-px.x,  px.y)).rgb * 0.0625;
    color += textureSample(t_color, s_color, in.uv + vec2<f32>( 0.0,  px.y)).rgb * 0.125;
    color += textureSample(t_color, s_color, in.uv + vec2<f32>( px.x,  px.y)).rgb * 0.0625;

    return vec4<f32>(color, 1.0);
}
