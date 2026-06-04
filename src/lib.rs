use std::sync::{Arc, atomic::{AtomicU32, Ordering}};
use wgpu::util::DeviceExt;
use winit::{
    event::*,
    event_loop::EventLoop,
    window::WindowBuilder,
    keyboard::{KeyCode, PhysicalKey},
};
use egui_wgpu::Renderer as EguiRenderer;
use egui_winit::State as EguiState;

mod wind_audio;
use wind_audio::WindAudioController;

thread_local! {
    static GLOBAL_ENV_STATE: Arc<EnvironmentState> = Arc::new(EnvironmentState::new());
}

pub struct EnvironmentState {
    wind_speed: AtomicU32,
    cloud_density: AtomicU32,
    sun_position: AtomicU32,
}

impl EnvironmentState {
    pub fn new() -> Self {
        Self {
            wind_speed: AtomicU32::new(1.0f32.to_bits()),
            cloud_density: AtomicU32::new(1.0f32.to_bits()),
            sun_position: AtomicU32::new(0.0f32.to_bits()),
        }
    }
    pub fn get_wind_speed(&self) -> f32 { f32::from_bits(self.wind_speed.load(Ordering::Relaxed)) }
    pub fn get_cloud_density(&self) -> f32 { f32::from_bits(self.cloud_density.load(Ordering::Relaxed)) }
    pub fn get_sun_position(&self) -> f32 { f32::from_bits(self.sun_position.load(Ordering::Relaxed)) }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct EnvironmentUniform {
    time: f32,
    wind_speed: f32,
    cloud_density: f32,
    sun_position: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    view_pos: [f32; 4],
    view_dir: [f32; 4],
    up: [f32; 4],
    right: [f32; 4],
    time: [f32; 4],
}

impl CameraUniform {
    fn new() -> Self {
        Self {
            view_pos: [0.0; 4],
            view_dir: [0.0; 4],
            up: [0.0; 4],
            right: [0.0; 4],
            time: [0.0; 4],
        }
    }

    fn update_view(&mut self, yaw: f32, pitch: f32, pos: glam::Vec3) {
        let view_dir = glam::Vec3::new(
            yaw.cos() * pitch.cos(),
            pitch.sin(),
            yaw.sin() * pitch.cos(),
        ).normalize();
        let global_up = glam::Vec3::new(0.0, 1.0, 0.0);
        let right = view_dir.cross(global_up).normalize();
        let up = right.cross(view_dir).normalize();
        
        self.view_pos = [pos.x, pos.y, pos.z, 0.0];
        self.view_dir = [view_dir.x, view_dir.y, view_dir.z, 0.0];
        self.up = [up.x, up.y, up.z, 0.0];
        self.right = [right.x, right.y, right.z, 0.0];
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct PostProcessUniforms {
    time: f32,
    _pad1: f32,
    resolution: [f32; 2],
    _pad2: [f32; 2],
}

pub struct State<'a> {
    surface: wgpu::Surface<'a>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    window: Arc<winit::window::Window>,
    
    render_pipeline: wgpu::RenderPipeline,
    render_bind_group: wgpu::BindGroup,
    post_process_pipeline: wgpu::RenderPipeline,
    post_process_bind_group: wgpu::BindGroup,
    post_process_bind_group_layout: wgpu::BindGroupLayout,
    post_process_uniform_buffer: wgpu::Buffer,
    render_target_texture: wgpu::Texture,
    render_target_view: wgpu::TextureView,
    history_texture: wgpu::Texture,
    history_view: wgpu::TextureView,
    screen_sampler: wgpu::Sampler,
    camera_uniform: CameraUniform,
    camera_buffer: wgpu::Buffer,
    env_uniform_buffer: wgpu::Buffer,
    compute_pipeline: wgpu::ComputePipeline,
    compute_bind_group: wgpu::BindGroup,
    audio_stream: Option<WindAudioController>,
    
    // Controles
    yaw: f32,
    pitch: f32,
    pos: glam::Vec3,
    
    w_pressed: bool,
    s_pressed: bool,
    a_pressed: bool,
    d_pressed: bool,
    q_pressed: bool,
    e_pressed: bool,
    
    mouse_pressed: bool,
    
    last_cloud_density: f32,
    last_sun_position: f32,
    
    // Egui
    egui_state: EguiState,
    egui_renderer: EguiRenderer,
    egui_context: egui::Context,
}

impl<'a> State<'a> {
    pub async fn new(window: Arc<winit::window::Window>) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        
        let surface = instance.create_surface(window.clone()).unwrap();
        
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }).await.unwrap();

        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
            },
            None,
        ).await.unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats.iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
            
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let volume_size = wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 64 };
        let volume_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Volume Texture"),
            size: volume_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let volume_view = volume_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let compute_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Compute Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/compute_noise.wgsl").into()),
        });

        let compute_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D3,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }
            ],
            label: Some("compute_bind_group_layout"),
        });

        let compute_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Compute Pipeline Layout"),
            bind_group_layouts: &[&compute_bind_group_layout],
            push_constant_ranges: &[],
        });

        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Compute Pipeline"),
            layout: Some(&compute_pipeline_layout),
            module: &compute_shader,
            entry_point: "main",
        });

        let env_uniform = EnvironmentUniform {
            time: 0.0,
            wind_speed: 1.0,
            cloud_density: 1.0,
            sun_position: 0.0,
        };
        let env_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Environment Uniform Buffer"),
            contents: bytemuck::cast_slice(&[env_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let compute_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Compute Bind Group"),
            layout: &compute_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&volume_view) },
                wgpu::BindGroupEntry { binding: 1, resource: env_uniform_buffer.as_entire_binding() },
            ],
        });

        let mut camera_uniform = CameraUniform::new();
        camera_uniform.update_view(-1.57, 0.2, glam::Vec3::new(0.0, 1.5, 0.0));
        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let render_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Render Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/volumetric.wgsl").into()),
        });

        let volume_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::MirrorRepeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::MirrorRepeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let render_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { multisampled: false, view_dimension: wgpu::TextureViewDimension::D3, sample_type: wgpu::TextureSampleType::Float { filterable: true } }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            ],
            label: Some("render_bind_group_layout"),
        });

        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&render_bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState { module: &render_shader, entry_point: "vs_main", buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &render_shader, entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState { format: config.format, blend: Some(wgpu::BlendState::REPLACE), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let render_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Render Bind Group"),
            layout: &render_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&volume_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&volume_sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: camera_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: env_uniform_buffer.as_entire_binding() },
            ],
        });

        let render_target_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Render Target Texture"),
            size: wgpu::Extent3d { width: config.width.max(2) / 2, height: config.height.max(2) / 2, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let render_target_view = render_target_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let history_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("History Texture"),
            size: wgpu::Extent3d { width: config.width.max(1), height: config.height.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: config.format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let history_view = history_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let screen_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let post_process_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Post Process Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/post_process.wgsl").into()),
        });

        let post_process_uniforms = PostProcessUniforms {
            time: 0.0,
            _pad1: 0.0,
            resolution: [config.width as f32, config.height as f32],
            _pad2: [0.0; 2],
        };
        let post_process_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Post Process Uniform Buffer"),
            contents: bytemuck::cast_slice(&[post_process_uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let post_process_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { multisampled: false, view_dimension: wgpu::TextureViewDimension::D2, sample_type: wgpu::TextureSampleType::Float { filterable: true } }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { multisampled: false, view_dimension: wgpu::TextureViewDimension::D2, sample_type: wgpu::TextureSampleType::Float { filterable: true } }, count: None },
            ],
            label: Some("post_process_bind_group_layout"),
        });

        let post_process_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Post Process Bind Group"),
            layout: &post_process_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&render_target_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&screen_sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: post_process_uniform_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&history_view) },
            ],
        });

        let post_process_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Post Process Pipeline Layout"),
            bind_group_layouts: &[&post_process_bind_group_layout],
            push_constant_ranges: &[],
        });

        let post_process_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Post Process Pipeline"),
            layout: Some(&post_process_pipeline_layout),
            vertex: wgpu::VertexState { module: &post_process_shader, entry_point: "vs_main", buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &post_process_shader, entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState { format: config.format, blend: Some(wgpu::BlendState::REPLACE), write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let audio_stream = Some(WindAudioController::new(GLOBAL_ENV_STATE.with(|s| s.clone())));

        // Egui initialization
        let egui_context = egui::Context::default();
        let viewport_id = egui_context.viewport_id();
        let egui_state = egui_winit::State::new(
            egui_context.clone(),
            viewport_id,
            &window,
            Some(window.scale_factor() as f32),
            None,
        );
        let egui_renderer = egui_wgpu::Renderer::new(&device, config.format, None, 1);

        let state = Self {
            surface,
            device,
            queue,
            config,
            size,
            window,
            render_pipeline,
            render_bind_group,
            post_process_pipeline,
            post_process_bind_group,
            post_process_bind_group_layout,
            post_process_uniform_buffer,
            render_target_texture,
            render_target_view,
            history_texture,
            history_view,
            screen_sampler,
            camera_uniform,
            camera_buffer,
            env_uniform_buffer,
            compute_pipeline,
            compute_bind_group,
            audio_stream,
            
            yaw: -1.57,
            pitch: -0.2,
            pos: glam::Vec3::new(0.0, 1.5, 0.0),
            w_pressed: false,
            s_pressed: false,
            a_pressed: false,
            d_pressed: false,
            q_pressed: false,
            e_pressed: false,
            mouse_pressed: false,
            
            last_cloud_density: -1.0,
            last_sun_position: -1.0,
            
            egui_state,
            egui_renderer,
            egui_context,
        };
        
        state.run_compute_pass();
        state
    }

    pub fn run_compute_pass(&self) {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("Compute Encoder") });
        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Compute Pass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&self.compute_pipeline);
            compute_pass.set_bind_group(0, &self.compute_bind_group, &[]);
            compute_pass.dispatch_workgroups(64 / 8, 64 / 8, 64 / 8);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            
            self.render_target_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Render Target Texture"),
                size: wgpu::Extent3d { width: self.config.width.max(2) / 2, height: self.config.height.max(2) / 2, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: self.config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.render_target_view = self.render_target_texture.create_view(&wgpu::TextureViewDescriptor::default());

            self.history_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("History Texture"),
                size: wgpu::Extent3d { width: self.config.width, height: self.config.height, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: self.config.format,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.history_view = self.history_texture.create_view(&wgpu::TextureViewDescriptor::default());
            
            let post_process_uniforms = PostProcessUniforms {
                time: self.camera_uniform.time[0],
                _pad1: 0.0,
                resolution: [self.size.width as f32, self.size.height as f32],
                _pad2: [0.0; 2],
            };
            self.queue.write_buffer(&self.post_process_uniform_buffer, 0, bytemuck::cast_slice(&[post_process_uniforms]));
            
            self.post_process_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Post Process Bind Group"),
                layout: &self.post_process_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&self.render_target_view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.screen_sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.post_process_uniform_buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.history_view) },
                ],
            });
        }
    }

    pub fn handle_mouse_move(&mut self, dx: f64, dy: f64) {
        if self.mouse_pressed {
            self.yaw -= dx as f32 * 0.005;
            self.pitch += dy as f32 * 0.005;
        }
    }

    pub fn render(&mut self, dt: f32) -> Result<(), wgpu::SurfaceError> {
        let speed = 0.08 * (dt * 60.0).max(0.1); 
        let forward = glam::Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        ).normalize();
        let right = forward.cross(glam::Vec3::new(0.0, 1.0, 0.0)).normalize();
        let up = glam::Vec3::new(0.0, 1.0, 0.0);

        if self.w_pressed { self.pos += forward * speed; }
        if self.s_pressed { self.pos -= forward * speed; }
        if self.d_pressed { self.pos += right * speed; }
        if self.a_pressed { self.pos -= right * speed; }
        if self.e_pressed { self.pos += up * speed; }
        if self.q_pressed { self.pos -= up * speed; }

        self.pos.y = self.pos.y.clamp(-0.5, 8.5);
        
        let mut current_wind_speed = GLOBAL_ENV_STATE.with(|s| s.get_wind_speed());
        let mut current_cloud_density = GLOBAL_ENV_STATE.with(|s| s.get_cloud_density());
        let mut current_sun_position = GLOBAL_ENV_STATE.with(|s| s.get_sun_position());
        
        // --- EGUI UI ---
        let raw_input = self.egui_state.take_egui_input(&self.window);
        let egui_output = self.egui_context.run(raw_input, |ctx| {
            egui::Window::new("Controles").show(ctx, |ui| {
                ui.label("Viento");
                if ui.add(egui::Slider::new(&mut current_wind_speed, 0.0..=5.0)).changed() {
                    GLOBAL_ENV_STATE.with(|s| s.wind_speed.store(current_wind_speed.to_bits(), Ordering::Relaxed));
                }
                
                ui.label("Sol (Hora)");
                if ui.add(egui::Slider::new(&mut current_sun_position, 0.0..=1.0)).changed() {
                    GLOBAL_ENV_STATE.with(|s| s.sun_position.store(current_sun_position.to_bits(), Ordering::Relaxed));
                }
                
                ui.label("Densidad Nubes");
                if ui.add(egui::Slider::new(&mut current_cloud_density, 0.0..=2.0)).changed() {
                    GLOBAL_ENV_STATE.with(|s| s.cloud_density.store(current_cloud_density.to_bits(), Ordering::Relaxed));
                }
            });
        });
        
        self.egui_state.handle_platform_output(&self.window, egui_output.platform_output);
        let paint_jobs = self.egui_context.tessellate(egui_output.shapes, self.egui_context.pixels_per_point());
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: self.window.scale_factor() as f32,
        };
        // ---------------

        self.camera_uniform.time[0] += dt * current_wind_speed;

        let env_uniform = EnvironmentUniform {
            time: self.camera_uniform.time[0],
            wind_speed: current_wind_speed,
            cloud_density: current_cloud_density,
            sun_position: current_sun_position,
        };
        self.queue.write_buffer(&self.env_uniform_buffer, 0, bytemuck::cast_slice(&[env_uniform]));
        
        if (self.last_cloud_density - current_cloud_density).abs() > 0.001 || 
           (self.last_sun_position - current_sun_position).abs() > 0.001 {
            self.last_cloud_density = current_cloud_density;
            self.last_sun_position = current_sun_position;
            self.run_compute_pass();
        }

        if let Some(audio) = &mut self.audio_stream {
            audio.update();
        }

        self.camera_uniform.update_view(self.yaw, self.pitch, self.pos);
        self.queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[self.camera_uniform]));
        
        let post_process_uniforms = PostProcessUniforms {
            time: self.camera_uniform.time[0],
            _pad1: 0.0,
            resolution: [self.size.width as f32, self.size.height as f32],
            _pad2: [0.0; 2],
        };
        self.queue.write_buffer(&self.post_process_uniform_buffer, 0, bytemuck::cast_slice(&[post_process_uniforms]));

        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("Render Encoder") });
        
        for (id, image_delta) in &egui_output.textures_delta.set {
            self.egui_renderer.update_texture(&self.device, &self.queue, *id, image_delta);
        }

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.render_target_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.render_bind_group, &[]);
            render_pass.draw(0..3, 0..1);
        }

        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );

        {
            let mut post_process_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Post Process Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            post_process_pass.set_pipeline(&self.post_process_pipeline);
            post_process_pass.set_bind_group(0, &self.post_process_bind_group, &[]);
            post_process_pass.draw(0..3, 0..1);
            
            self.egui_renderer.render(&mut post_process_pass, &paint_jobs, &screen_descriptor);
        }

        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture { texture: &output.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::ImageCopyTexture { texture: &self.history_texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::Extent3d { width: self.size.width, height: self.size.height, depth_or_array_layers: 1 }
        );

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        
        for id in &egui_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
        
        Ok(())
    }
}

pub fn run() {
    #[cfg(target_arch = "wasm32")]
    {
        std::panic::set_hook(Box::new(console_error_panic_hook::hook));
        console_log::init_with_level(log::Level::Warn).expect("Couldn't initialize logger");
    }

    let event_loop = EventLoop::new().unwrap();
    let window = Arc::new(WindowBuilder::new().with_title("Nubes").build(&event_loop).unwrap());
    
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::WindowExtWebSys;
        let canvas = window.canvas().unwrap();
        let web_window = web_sys::window().unwrap();
        let document = web_window.document().unwrap();
        let body = document.body().unwrap();
        body.append_child(&canvas).expect("Append canvas to HTML body");
        
        wasm_bindgen_futures::spawn_local(run_async(event_loop, window));
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        pollster::block_on(run_async(event_loop, window));
    }
}

async fn run_async(event_loop: EventLoop<()>, window: Arc<winit::window::Window>) {
    let mut state = State::new(window.clone()).await;
    let mut last_time = web_time::Instant::now();
    
    event_loop.run(move |event, elwt| {
        elwt.set_control_flow(winit::event_loop::ControlFlow::Poll);
        match event {
            Event::WindowEvent { ref event, window_id } if window_id == window.id() => {
                let egui_consumed = state.egui_state.on_window_event(&window, event).consumed;
                
                if !egui_consumed {
                    match event {
                        WindowEvent::CloseRequested => elwt.exit(),
                        WindowEvent::Resized(physical_size) => state.resize(*physical_size),
                        WindowEvent::KeyboardInput {
                            event: winit::event::KeyEvent { physical_key: PhysicalKey::Code(keycode), state: element_state, .. },
                            ..
                        } => {
                            let is_pressed = *element_state == ElementState::Pressed;
                            match keycode {
                                KeyCode::KeyW => state.w_pressed = is_pressed,
                                KeyCode::KeyA => state.a_pressed = is_pressed,
                                KeyCode::KeyS => state.s_pressed = is_pressed,
                                KeyCode::KeyD => state.d_pressed = is_pressed,
                                KeyCode::KeyQ => state.q_pressed = is_pressed,
                                KeyCode::KeyE => state.e_pressed = is_pressed,
                                _ => {}
                            }
                        }
                        WindowEvent::MouseInput { state: element_state, button: MouseButton::Left, .. } => {
                            state.mouse_pressed = *element_state == ElementState::Pressed;
                        }
                        _ => {}
                    }
                }
            }
            Event::DeviceEvent { event: DeviceEvent::MouseMotion { delta }, .. } => {
                if state.mouse_pressed {
                    state.handle_mouse_move(delta.0, delta.1);
                }
            }
            Event::AboutToWait => {
                window.request_redraw();
            }
            Event::WindowEvent { event: WindowEvent::RedrawRequested, .. } => {
                let now = web_time::Instant::now();
                let dt = now.duration_since(last_time).as_secs_f32();
                last_time = now;
                
                match state.render(dt) {
                    Ok(_) => {}
                    Err(wgpu::SurfaceError::Lost) => state.resize(state.size),
                    Err(wgpu::SurfaceError::OutOfMemory) => elwt.exit(),
                    Err(e) => eprintln!("{:?}", e),
                }
            }
            _ => {}
        }
    }).unwrap();
}
