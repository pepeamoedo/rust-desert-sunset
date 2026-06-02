@group(0) @binding(0) var volume_tex: texture_3d<f32>;
@group(0) @binding(1) var volume_sampler: sampler;

struct CameraUniform {
    view_pos: vec3<f32>,
    _padding1: f32,
    view_dir: vec3<f32>,
    _padding2: f32,
    up: vec3<f32>,
    _padding3: f32,
    right: vec3<f32>,
    _padding4: f32,
    time: vec4<f32>,
};
@group(0) @binding(2) var<uniform> camera: CameraUniform;

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

// Intersect ray with a horizontal slab (y from yMin to yMax)
fn intersectSlab(ro: vec3<f32>, rd: vec3<f32>, yMin: f32, yMax: f32) -> vec2<f32> {
    if (abs(rd.y) < 0.0001) {
        if (ro.y >= yMin && ro.y <= yMax) {
            return vec2<f32>(0.0, 1000.0);
        }
        return vec2<f32>(0.0, 0.0);
    }
    var t0 = (yMin - ro.y) / rd.y;
    var t1 = (yMax - ro.y) / rd.y;
    if (t0 > t1) {
        let temp = t0;
        t0 = t1;
        t1 = temp;
    }
    let tmin = max(0.0, t0);
    let tmax = t1;
    if (tmin > tmax || tmax < 0.0) {
        return vec2<f32>(0.0, 0.0);
    }
    return vec2<f32>(tmin, tmax - tmin);
}

// Intersect ray with a sphere
fn intersectSphere(ro: vec3<f32>, rd: vec3<f32>, center: vec3<f32>, radius: f32) -> f32 {
    let oc = ro - center;
    let b = dot(oc, rd);
    let c = dot(oc, oc) - radius * radius;
    let h = b * b - c;
    if (h < 0.0) {
        return -1.0;
    }
    return -b - sqrt(h);
}

// Henyey-Greenstein phase function
fn phase_hg(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    let pi = 3.14159265;
    return (1.0 - g2) / (4.0 * pi * pow(1.0 + g2 - 2.0 * g * cos_theta, 1.5));
}

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

// Ridged multifractal noise for sharp crests (barchan/transverse dunes)
fn ridged_fbm(p: vec3<f32>) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var shift = vec3<f32>(100.0);
    var p_mut = p;
    for (var i = 0; i < 4; i++) {
        // Map noise to [-0.5, 0.5], take absolute value, multiply by 2 to get [0, 1]
        // Then invert it so the sharp valleys become sharp crests
        let n = 1.0 - abs(noise(p_mut) - 0.5) * 2.0; 
        // Raise to a higher power (e.g. cubed) to make the crests extremely thin and pronounced
        v += a * (n * n * n); 
        p_mut = p_mut * 2.1 + shift;
        a *= 0.5;
    }
    return v;
}

// Terrain SDF
fn mapTerrain(p: vec3<f32>) -> f32 {
    let sea_level = -1.0;
    // Increase frequency so islands are closer to each other
    let island_noise = fbm(vec3<f32>(p.x * 0.06, 0.0, p.z * 0.06) + vec3<f32>(12.3, 0.0, 4.5));
    // Lower the terrain so islands are less frequent and have lower peaks
    // Allow the terrain to drop below sea level so the water plane naturally intersects it
    let terrain_h = sea_level + island_noise * 10.0 - 5.5;
    return p.y - terrain_h;
}

fn raymarchTerrain(ro: vec3<f32>, rd: vec3<f32>) -> f32 {
    var t = 0.0;
    for (var i = 0; i < 160; i++) { // Increased iterations to reach further
        let p = ro + rd * t;
        let d = mapTerrain(p);
        if (d < 0.01) {
            return t;
        }
        t += d * 0.8; // Faster stepping
        if (t > 800.0) { // Render distance pushed way back
            break;
        }
    }
    return -1.0;
}

fn getTerrainNormal(p: vec3<f32>) -> vec3<f32> {
    let e = vec2<f32>(0.01, 0.0);
    let n = vec3<f32>(
        mapTerrain(p + e.xyy) - mapTerrain(p - e.xyy),
        mapTerrain(p + e.yxy) - mapTerrain(p - e.yxy),
        mapTerrain(p + e.yyx) - mapTerrain(p - e.yyx)
    );
    return normalize(n);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv * 2.0 - 1.0;
    
    // Wide angle lens with slight barrel distortion (GoPro effect)
    let ro = camera.view_pos;
    var wuv = uv;
    wuv *= 1.0 + 0.15 * dot(uv, uv); 
    let rd = normalize(camera.right * wuv.x * 1.4 + camera.up * -wuv.y * 1.4 + camera.view_dir);
    
    // Light setup: Sunset
    let sun_dir = normalize(vec3<f32>(0.9, 0.08, 0.4)); // Sun very low on the horizon
    let sun_color = vec3<f32>(1.0, 0.4, 0.1) * 8.0; // Intense orange/red radiant sun
    
    // Sunset desert sky palette
    let sky_color_bottom = vec3<f32>(0.9, 0.25, 0.1); // Fiery red/orange horizon
    let sky_color_top = vec3<f32>(0.15, 0.1, 0.25);    // Dark purple twilight above
    
    // Background sky gradient
    let sky_bg = mix(sky_color_bottom, sky_color_top, max(0.0, rd.y));
    let sun_dot = max(0.0, dot(rd, sun_dir));
    var sky_color = sky_bg + sun_color * pow(sun_dot, 500.0) + sun_color * pow(sun_dot, 50.0) * 0.2;
    
    // --- Stars & Venus ---
    if (rd.y > 0.0) {
        // Calculate a slightly parallaxed ray direction for the celestial sphere
        // This gives the illusion that the starry sky is rotating slowly relative to the camera
        let cam_yaw = atan2(camera.view_dir.x, camera.view_dir.z);
        let sky_parallax = 0.15; // 15% parallax effect
        let rot_angle = -cam_yaw * sky_parallax;
        let cos_r = cos(rot_angle);
        let sin_r = sin(rot_angle);
        
        let sky_rd = vec3<f32>(
            rd.x * cos_r - rd.z * sin_r,
            rd.y,
            rd.x * sin_r + rd.z * cos_r
        );

        // Fixed stars using a 3D grid (cellular/Voronoi) mapped to the celestial sphere
        let grid_size = 200.0;
        let cell = floor(sky_rd * grid_size);
        let cell_center = (cell + 0.5) / grid_size;
        let star_rand = hash3(cell);
        
        // Random fixed position for the star within this grid cell
        let star_pos = normalize(cell_center + (star_rand - 0.5) * 0.8 / grid_size);
        
        // Distance from current viewing ray to the star's fixed position
        let dist = distance(sky_rd, star_pos);
        
        // Only ~8% of cells have a star (sparser sky)
        let has_star = step(0.92, star_rand.y);
        
        // Draw the star (a much smaller, sharper circle for realism)
        let star_intensity = smoothstep(0.0008, 0.0001, dist) * has_star;
        
        // Add random color variation (blueish to reddish)
        let star_color = mix(vec3<f32>(0.7, 0.9, 1.0), vec3<f32>(1.0, 0.8, 0.6), star_rand.z);
        
        // Fade stars near the bright horizon
        let star_fade = smoothstep(0.1, 0.5, sky_rd.y) * (1.0 - sun_dot);
        let stars = star_color * star_intensity * 4.0 * star_fade;
        
        // Venus (El lucero del alba/atardecer)
        // Placed high up, to the left of the sun
        let venus_dir = normalize(vec3<f32>(0.6, 0.45, 0.8));
        let venus_dot = dot(sky_rd, venus_dir);
        // A sharp core and a soft glow
        let venus_core = smoothstep(0.99995, 1.0, venus_dot) * 10.0;
        let venus_glow = pow(max(0.0, venus_dot), 8000.0) * 2.0;
        let venus = vec3<f32>(1.0, 0.95, 0.8) * (venus_core + venus_glow);
        
        // Moon (Waning crescent / Luna menguante)
        // Placed somewhat opposite to the sun
        let moon_dir = normalize(vec3<f32>(0.1, 0.55, 0.9));
        let moon_dist = acos(clamp(dot(sky_rd, moon_dir), -1.0, 1.0));
        let moon_radius = 0.035;
        
        // Full moon base (for earthshine)
        let moon_base = smoothstep(moon_radius, moon_radius - 0.0015, moon_dist);
        
        // Shifted shadow to create the crescent cut-out
        // The shadow offset defines the phase of the moon
        let shadow_offset = vec3<f32>(0.015, -0.005, -0.01);
        let moon_shadow_dir = normalize(moon_dir + shadow_offset);
        let shadow_dist = acos(clamp(dot(sky_rd, moon_shadow_dir), -1.0, 1.0));
        let moon_shadow = smoothstep(moon_radius, moon_radius - 0.005, shadow_dist);
        
        // The lit crescent
        let moon_crescent = clamp(moon_base - moon_shadow, 0.0, 1.0);
        // "Earthshine" (Luz cenicienta) illuminates the dark side faintly
        let earthshine = moon_base * 0.08; 
        
        let moon_color = vec3<f32>(0.85, 0.9, 1.0);
        let moon = moon_color * (moon_crescent + earthshine);
        
        sky_color += stars + venus + moon;
    }
    
    var bg_color = sky_color;
    var t_sea = raymarchTerrain(ro, rd);
    
    if (t_sea > 0.0) {
        let p_terrain = ro + rd * t_sea;
        var n_terrain = getTerrainNormal(p_terrain);
        let sea_level = -1.0;
        let is_floor = p_terrain.y <= sea_level + 0.05;
        
        var base_color = vec3<f32>(0.0);
        
        // Cloud shadows removed (at sunset they stretch infinitely and look unrealistic, removing them boosts performance)
        let cloud_shadow_factor = 1.0;
        
        if (is_floor) {
            // Infinite Desert Floor with Bump Mapping for 3D Dunes
            let dune_freq = 0.03;
            // Use ridged noise to create pronounced crests
            let d_center = ridged_fbm(p_terrain * dune_freq);
            
            // Compute gradients for bump mapping using the same ridged noise
            let eps = vec2<f32>(0.5, 0.0);
            let d_dx = ridged_fbm((p_terrain + eps.xyy) * dune_freq);
            let d_dz = ridged_fbm((p_terrain + eps.yyx) * dune_freq);
            
            // Perturb normal to simulate 3D dune slopes
            // The magic number controls the height/steepness of the dunes
            let bump_normal = normalize(n_terrain + vec3<f32>(d_center - d_dx, 0.1, d_center - d_dz) * 35.0);
            
            let ripple_warp = sin(p_terrain.z * 5.0) * 0.5;
            let ripples = sin((p_terrain.x + ripple_warp) * 30.0) * 0.02;
            let grain = noise(p_terrain * 80.0) * 0.03;
            
            // Uniform African desert sand color
            let light_sand = vec3<f32>(0.90, 0.75, 0.45); // Warm, pale desert sand
            
            // Shadows derive dynamically from the local sand color, making them cooler and darker
            // At sunset, shadows become very long, dark, and purple/blueish
            let shadow_sand = light_sand * 0.15 + vec3<f32>(0.05, 0.05, 0.15);  
            var dune_color = mix(shadow_sand, light_sand, d_center);
            
            dune_color = dune_color + ripples + grain;
            
            // Calculate long self-shadowing of the dunes by marching towards the low sun
            var terrain_shadow = 1.0;
            let shadow_step = 1.5;
            for (var i = 1; i <= 8; i++) {
                let p_sample = p_terrain + sun_dir * (f32(i) * shadow_step);
                let h_sample = ridged_fbm(p_sample * dune_freq);
                
                // The ray's virtual height increases as it travels towards the sun
                let ray_h = d_center + (sun_dir.y * f32(i) * shadow_step) * 0.15; 
                
                if (h_sample > ray_h) {
                    // Soften the shadow based on how deep the ray hits the dune
                    terrain_shadow -= (h_sample - ray_h) * 5.0;
                }
            }
            terrain_shadow = clamp(terrain_shadow, 0.0, 1.0);
            
            // Calculate basic diffuse using the bump normal, then apply the long shadow
            let raw_diffuse = max(dot(bump_normal, sun_dir), 0.0) * 0.85 + 0.15;
            let diffuse = raw_diffuse * mix(0.2, 1.0, terrain_shadow);
            
            // Sparkles on the sand (specular highlight of quartz grains)
            let view_dir = -rd;
            let half_vec = normalize(sun_dir + view_dir);
            let specular_base = max(dot(bump_normal, half_vec), 0.0);
            
            // High frequency noise mask so it glitters like grains instead of shining like plastic
            let sparkle_noise = noise(p_terrain * 250.0);
            let sparkle_mask = smoothstep(0.85, 1.0, sparkle_noise);
            // Strong specular power so the dots are very sharp, and multiply by terrain shadow so they don't shine in the dark
            let sparkle = pow(specular_base, 80.0) * sparkle_mask * 3.0 * cloud_shadow_factor * terrain_shadow;
            
            base_color = clamp(dune_color * diffuse * cloud_shadow_factor + vec3<f32>(sparkle), vec3<f32>(0.0), vec3<f32>(1.0));
        } else {
            // Desert Rock Formations
            let height_above_sea = p_terrain.y - sea_level;
            let slope = 1.0 - max(dot(n_terrain, vec3<f32>(0.0, 1.0, 0.0)), 0.0);
            
            let sand_color = vec3<f32>(0.85, 0.70, 0.50);
            let dirt_color = vec3<f32>(0.75, 0.55, 0.40);
            let rock_color = vec3<f32>(0.65, 0.45, 0.35); 
            
            var terrain_col = vec3<f32>(0.0);
            if (height_above_sea < 0.5) {
                terrain_col = mix(sand_color, dirt_color, smoothstep(0.1, 0.5, height_above_sea));
            } else {
                terrain_col = mix(dirt_color, rock_color, smoothstep(0.0, 0.4, slope));
            }
            
            let diffuse = max(dot(n_terrain, sun_dir), 0.0) * 0.8 + 0.2;
            base_color = terrain_col * diffuse * cloud_shadow_factor;
        }
        
        // Sfumato matches the sky perfectly to hide the rendering edge
        // Density tuned so it fades smoothly exactly at the new 800.0 limit
        let fog_density = 0.004; 
        let fog_factor = clamp(1.0 - exp(-t_sea * fog_density), 0.0, 1.0);
        bg_color = mix(base_color, sky_color, fog_factor);
    }
    
    let ray_slab_info = intersectSlab(ro, rd, 2.0, 8.0);
    let dstToBox = ray_slab_info.x;
    let rawDstInsideBox = ray_slab_info.y;
    
    // Cap the distance so we don't raymarch to infinity when looking at the horizon
    let dstInsideBox = min(rawDstInsideBox, 40.0);
    
    var final_color = bg_color;
    
    if (dstInsideBox > 0.0) {
        let num_steps = 64; // Optimized from 128
        let step_size = dstInsideBox / f32(num_steps);
        var p = ro + rd * dstToBox;
        
        var transmittance = 1.0;
        let absorption = 12.0;
        var scattered_light = vec3<f32>(0.0);
        
        // Phase function evaluation for current ray direction and sun direction
        let cos_theta = dot(rd, sun_dir);
        // Mix two HG phases for strong forward scattering and slight back scattering
        let phase_val = mix(phase_hg(cos_theta, 0.8), phase_hg(cos_theta, -0.2), 0.3);
        
        let shadow_steps = 4; // Optimized from 6
        let shadow_step_size = 0.08;
        
        let wind = vec3<f32>(camera.time.x * 0.5, 0.0, camera.time.x * 0.2);
        
        for (var i = 0; i < num_steps; i++) {
            let p_wind = p + wind;
            // Domain Warping: Distort the sampling coordinates organically
            let warp = vec3<f32>(sin(p_wind.z * 0.7), 0.0, cos(p_wind.x * 0.6)) * 0.6;
            // Scale XZ by 0.2, and Y by 0.16 to fit the new 6.0 height slab into the 1.0 texture height
            let sample_p = vec3<f32>((p_wind.x + warp.x) * 0.2, (p.y - 2.0) / 6.0, (p_wind.z + warp.z) * 0.2);
            
            // Macro mask: very low frequency trig functions to hide repetition
            let mask = sin(p_wind.x * 0.25) * cos(p_wind.z * 0.15) * 0.5 + 0.5;
            
            let raw_density = textureSampleLevel(volume_tex, volume_sampler, sample_p, 0.0).r;
            // Subtract the mask so some areas are artificially clear, breaking the grid
            let density = max(0.0, raw_density - (1.0 - mask) * 0.6);
            
            if (density > 0.01) {
                // Secondary raycast towards the sun to calculate shadowing
                var shadow_density = 0.0;
                var light_p = p + sun_dir * shadow_step_size;
                for (var j = 0; j < shadow_steps; j++) {
                    let l_wind = light_p + wind;
                    let l_warp = vec3<f32>(sin(l_wind.z * 0.7), 0.0, cos(l_wind.x * 0.6)) * 0.6;
                    let l_sample_p = vec3<f32>((l_wind.x + l_warp.x) * 0.2, (light_p.y - 2.0) / 6.0, (l_wind.z + l_warp.z) * 0.2);
                    let l_mask = sin(l_wind.x * 0.25) * cos(l_wind.z * 0.15) * 0.5 + 0.5;
                    
                    let d = max(0.0, textureSampleLevel(volume_tex, volume_sampler, l_sample_p, 0.0).r - (1.0 - l_mask) * 0.6);
                    shadow_density += d;
                    light_p += sun_dir * shadow_step_size;
                }
                
                // Beer-Lambert law for light reaching this point
                let light_transmittance = exp(-shadow_density * shadow_step_size * absorption);
                
                // Light scattered towards the camera
                let step_transmittance = exp(-density * step_size * absorption);
                transmittance *= step_transmittance;
                
                // Add lighting
                let bounce_color = vec3<f32>(0.6, 0.2, 0.05); // Deep red/orange sand bounce light for sunset
                let bounce_intensity = smoothstep(8.0, 2.0, p.y); // Stronger at the bottom of the clouds
                let bounce_light = bounce_color * bounce_intensity * 0.3;
                
                let ambient_light = vec3<f32>(0.15, 0.1, 0.2) * 0.5 + bounce_light; // Purple twilight ambient fill + bounce
                let direct_light = sun_color * light_transmittance * phase_val;
                
                let luminance = density * step_size * absorption;
                scattered_light += (direct_light + ambient_light) * luminance * transmittance;
            }
            
            if (transmittance < 0.01) {
                break;
            }
            
            p += rd * step_size;
        }
        
        // Mix the clouds with the background sky based on remaining transmittance
        final_color = scattered_light + bg_color * transmittance;
    }
    
    // --- Lens Flare (Fantasmas de lente) ---
    let sun_uv_x = dot(sun_dir, camera.right);
    let sun_uv_y = -dot(sun_dir, camera.up);
    let sun_z = dot(sun_dir, camera.view_dir);
    
    if (sun_z > 0.0) {
        // Project sun to screen coordinates
        let sun_uv = vec2<f32>(sun_uv_x, sun_uv_y) / sun_z;
        let ghost_vec = -sun_uv; // Vector pointing to the opposite side of the center
        
        var flare = vec3<f32>(0.0);
        
        // Ghost 1 (Large Cyan)
        let g1_pos = ghost_vec * 0.4;
        let d1 = length(uv - g1_pos);
        flare += vec3<f32>(0.1, 0.5, 0.8) * (0.03 / (d1 + 0.02)) * smoothstep(0.8, 0.0, d1);
        
        // Ghost 2 (Small Orange/Red)
        let g2_pos = ghost_vec * 1.1;
        let d2 = length(uv - g2_pos);
        flare += vec3<f32>(0.8, 0.3, 0.1) * (0.005 / (d2 + 0.005)) * smoothstep(0.2, 0.0, d2);
        
        // Fade flare out when looking away from the sun
        let sun_dist = length(sun_uv);
        flare *= smoothstep(1.8, 0.5, sun_dist);
        
        final_color += flare;
    }
    
    // Simple tone mapping (ACES-like approximation) applied universally
    let mapped_color = final_color / (final_color + vec3<f32>(1.0));
    
    return vec4<f32>(pow(mapped_color, vec3<f32>(1.0 / 2.2)), 1.0);
}
