@group(0) @binding(0) var volume: texture_storage_3d<rgba8unorm, write>;

// Simple hash function for noise
fn hash3(p: vec3<f32>) -> vec3<f32> {
    var q = vec3<f32>(
        dot(p, vec3<f32>(127.1, 311.7, 74.7)),
        dot(p, vec3<f32>(269.5, 183.3, 246.1)),
        dot(p, vec3<f32>(113.5, 271.9, 124.6))
    );
    return fract(sin(q) * 43758.5453123);
}

// Simple 3D gradient noise
fn noise(x: vec3<f32>) -> f32 {
    let p = floor(x);
    let f = fract(x);

    let u = f * f * (3.0 - 2.0 * f);

    let n = p;
    let v000 = dot(hash3(n + vec3<f32>(0.0, 0.0, 0.0)) - 0.5, f - vec3<f32>(0.0, 0.0, 0.0));
    let v100 = dot(hash3(n + vec3<f32>(1.0, 0.0, 0.0)) - 0.5, f - vec3<f32>(1.0, 0.0, 0.0));
    let v010 = dot(hash3(n + vec3<f32>(0.0, 1.0, 0.0)) - 0.5, f - vec3<f32>(0.0, 1.0, 0.0));
    let v110 = dot(hash3(n + vec3<f32>(1.0, 1.0, 0.0)) - 0.5, f - vec3<f32>(1.0, 1.0, 0.0));
    let v001 = dot(hash3(n + vec3<f32>(0.0, 0.0, 1.0)) - 0.5, f - vec3<f32>(0.0, 0.0, 1.0));
    let v101 = dot(hash3(n + vec3<f32>(1.0, 0.0, 1.0)) - 0.5, f - vec3<f32>(1.0, 0.0, 1.0));
    let v011 = dot(hash3(n + vec3<f32>(0.0, 1.0, 1.0)) - 0.5, f - vec3<f32>(0.0, 1.0, 1.0));
    let v111 = dot(hash3(n + vec3<f32>(1.0, 1.0, 1.0)) - 0.5, f - vec3<f32>(1.0, 1.0, 1.0));

    let x00 = mix(v000, v100, u.x);
    let x10 = mix(v010, v110, u.x);
    let x01 = mix(v001, v101, u.x);
    let x11 = mix(v011, v111, u.x);

    let y0 = mix(x00, x10, u.y);
    let y1 = mix(x01, x11, u.y);

    return mix(y0, y1, u.z) + 0.5;
}

fn fbm(p: vec3<f32>) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var shift = vec3<f32>(100.0);
    var p_mut = p;
    for (var i = 0; i < 4; i++) {
        v += a * noise(p_mut);
        p_mut = p_mut * 2.0 + shift;
        a *= 0.5;
    }
    return v;
}

@compute @workgroup_size(4, 4, 4)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(volume);
    if (id.x >= size.x || id.y >= size.y || id.z >= size.z) {
        return;
    }

    let pos = vec3<f32>(id) / vec3<f32>(size);
    
    // Macro structure: large scale noise to place cloud clusters
    let macro_noise = fbm(pos * 3.0 + vec3<f32>(12.3, 4.5, 6.7));
    
    // Generate a varying height limit for clouds using 2D noise
    let height_noise = fbm(vec3<f32>(pos.x * 2.5, 0.0, pos.z * 2.5) + vec3<f32>(7.1, 0.0, 3.3));
    
    // Layer 1 (Lower, Sparse): 0.0 to ~0.35
    let max_height_1 = mix(0.1, 0.35, height_noise);
    let l1_bottom = smoothstep(0.0, 0.05, pos.y);
    let l1_top = smoothstep(max_height_1, max_height_1 - 0.1, pos.y);
    let mask_l1 = l1_bottom * l1_top;
    
    // Layer 2 (Higher, Dense/Less sparse): 0.55 to ~1.0
    let max_height_2 = mix(0.7, 1.0, height_noise);
    let l2_bottom = smoothstep(0.55, 0.6, pos.y);
    let l2_top = smoothstep(max_height_2, max_height_2 - 0.1, pos.y);
    let mask_l2 = l2_bottom * l2_top;
    
    // Create the base shapes with different sparsity thresholds
    // Lower layer is sparser (higher cutoff)
    let base_shape_1 = smoothstep(0.4, 0.8, macro_noise) * mask_l1;
    // Higher layer is less sparse (lower cutoff, wider coverage)
    let base_shape_2 = smoothstep(0.1, 0.6, macro_noise) * mask_l2;
    
    let base_shape = base_shape_1 + base_shape_2;

    // High frequency noise for the fluffy details and erosion
    let detail_noise = fbm(pos * 12.0);
    
    // Create harsh contrast for more chaotic, dense gas clumps (lower erosion multiplier = thicker clouds)
    let density = max(0.0, base_shape - (1.0 - detail_noise) * 0.65);

    // ==========================================
    // SHADOW BAKING
    // ==========================================
    var light_transmittance = 1.0;
    
    if (density > 0.01) {
        let sun_dir = normalize(vec3<f32>(0.9, 0.08, 0.4));
        var shadow_density = 0.0;
        let shadow_step_size = 0.05;
        var shadow_pos = pos + sun_dir * shadow_step_size;
        
        for (var i = 0; i < 8; i++) {
            if (shadow_pos.x > 1.0 || shadow_pos.y > 1.0 || shadow_pos.z > 1.0 ||
                shadow_pos.x < 0.0 || shadow_pos.y < 0.0 || shadow_pos.z < 0.0) {
                break;
            }
            
            let s_macro = fbm(shadow_pos * 3.0 + vec3<f32>(12.3, 4.5, 6.7));
            let s_height = fbm(vec3<f32>(shadow_pos.x * 2.5, 0.0, shadow_pos.z * 2.5) + vec3<f32>(7.1, 0.0, 3.3));
            
            let s_max1 = mix(0.1, 0.35, s_height);
            let s_mask1 = smoothstep(0.0, 0.05, shadow_pos.y) * smoothstep(s_max1, s_max1 - 0.1, shadow_pos.y);
            let s_max2 = mix(0.7, 1.0, s_height);
            let s_mask2 = smoothstep(0.55, 0.6, shadow_pos.y) * smoothstep(s_max2, s_max2 - 0.1, shadow_pos.y);
            
            let s_base = smoothstep(0.4, 0.8, s_macro) * s_mask1 + smoothstep(0.1, 0.6, s_macro) * s_mask2;
            let s_detail = fbm(shadow_pos * 12.0);
            let s_d = max(0.0, s_base - (1.0 - s_detail) * 0.65);
            
            shadow_density += s_d;
            shadow_pos += sun_dir * shadow_step_size;
        }
        light_transmittance = exp(-shadow_density * 8.0); // absorption factor
    }

    let color = vec4<f32>(density, light_transmittance, 0.0, 1.0);
    textureStore(volume, id, color);
}
