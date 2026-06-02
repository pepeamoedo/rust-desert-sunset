#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

mod wind_audio;
use rodio::{OutputStream, Sink};
use std::time::Instant;
use egui::Context as EguiContext;
use egui_winit::State as EguiState;
use egui_wgpu::Renderer as EguiRenderer;

use wgpu::util::DeviceExt;
use bytemuck::{Pod, Zeroable};

use winit::{
    event::*,
    event_loop::EventLoop,
    window::WindowBuilder,
    keyboard::{KeyCode, PhysicalKey},
};

const VOL_SIZE: u32 = 128;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
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
        // Panoramic camera sitting above the cloud slab
        
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
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct PostProcessUniforms {
    time: f32,
    _pad1: f32,
    resolution: [f32; 2],
    _pad2: [f32; 2],
}

struct State<'a> {
    surface: wgpu::Surface<'a>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    window: &'a winit::window::Window,
    render_pipeline: wgpu::RenderPipeline,
    render_bind_group: wgpu::BindGroup,
    post_process_pipeline: wgpu::RenderPipeline,
    post_process_bind_group: wgpu::BindGroup,
    post_process_bind_group_layout: wgpu::BindGroupLayout,
    post_process_uniform_buffer: wgpu::Buffer,
    render_target_texture: wgpu::Texture,
    render_target_view: wgpu::TextureView,
    screen_sampler: wgpu::Sampler,
    camera_uniform: CameraUniform,
    camera_buffer: wgpu::Buffer,
    is_dragging: bool,
    yaw: f32,
    pitch: f32,
    pos: glam::Vec3,
    w_pressed: bool,
    s_pressed: bool,
    a_pressed: bool,
    d_pressed: bool,
    q_pressed: bool,
    e_pressed: bool,
    last_mouse_pos: (f64, f64),
    audio_stream: Option<(OutputStream, Sink)>,
    egui_ctx: EguiContext,
    egui_state: EguiState,
    egui_renderer: EguiRenderer,
    gpu_name: String,
    last_frame_time: Instant,
    fps_history: Vec<f32>,
    show_perf_panel: bool,
}

impl<'a> State<'a> {
    async fn new(window: &'a winit::window::Window) -> State<'a> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        
        let surface = instance.create_surface(window).unwrap();

        let adapter = instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            },
        ).await.unwrap();

        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                label: None,
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
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let volume_texture_extent = wgpu::Extent3d {
            width: VOL_SIZE,
            height: VOL_SIZE,
            depth_or_array_layers: VOL_SIZE,
        };
        
        let volume_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Volume Texture"),
            size: volume_texture_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });

        let volume_view = volume_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let compute_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Compute Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/compute_noise.wgsl").into()),
        });

        let compute_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    view_dimension: wgpu::TextureViewDimension::D3,
                },
                count: None,
            }],
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

        let compute_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Compute Bind Group"),
            layout: &compute_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&volume_view),
            }],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("Compute Encoder") });
        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Compute Pass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&compute_pipeline);
            compute_pass.set_bind_group(0, &compute_bind_group, &[]);
            compute_pass.dispatch_workgroups(VOL_SIZE / 4, VOL_SIZE / 4, VOL_SIZE / 4);
        }
        queue.submit(std::iter::once(encoder.finish()));

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
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D3,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
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
            vertex: wgpu::VertexState {
                module: &render_shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &render_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        });

        let render_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Render Bind Group"),
            layout: &render_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&volume_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&volume_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: camera_buffer.as_entire_binding(),
                },
            ],
        });

        let render_target_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Render Target Texture"),
            size: wgpu::Extent3d {
                width: config.width / 2, // Half resolution for performance
                height: config.height / 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let render_target_view = render_target_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let screen_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let post_process_uniforms = PostProcessUniforms {
            time: 0.0,
            _pad1: 0.0,
            resolution: [config.width as f32, config.height as f32],
            _pad2: [0.0; 2],
        };
        let post_process_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Post Process Uniforms"),
            contents: bytemuck::cast_slice(&[post_process_uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let post_process_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Post Process Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/post_process.wgsl").into()),
        });

        let post_process_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
            label: Some("post_process_bind_group_layout"),
        });

        let post_process_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Post Process Bind Group"),
            layout: &post_process_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&render_target_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&screen_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: post_process_uniform_buffer.as_entire_binding(),
                },
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
            vertex: wgpu::VertexState {
                module: &post_process_shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &post_process_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let gpu_name = adapter.get_info().name.clone();

        let egui_ctx = EguiContext::default();
        let viewport_id = egui_ctx.viewport_id();
        let egui_state = EguiState::new(
            egui_ctx.clone(),
            viewport_id,
            window,
            Some(window.scale_factor() as f32),
            None,
        );
        let egui_renderer = EguiRenderer::new(&device, config.format, None, 1);

        Self {
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
            screen_sampler,
            camera_uniform,
            camera_buffer,
            is_dragging: false,
            yaw: -1.57,
            pitch: -0.2,
            pos: glam::Vec3::new(0.0, 1.5, 0.0),
            w_pressed: false,
            s_pressed: false,
            a_pressed: false,
            d_pressed: false,
            q_pressed: false,
            e_pressed: false,
            last_mouse_pos: (0.0, 0.0),
            audio_stream: None,
            egui_ctx,
            egui_state,
            egui_renderer,
            gpu_name,
            last_frame_time: Instant::now(),
            fps_history: Vec::new(),
            show_perf_panel: false,
        }
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            
            // Recreate render target for the new size
            self.render_target_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Render Target Texture"),
                size: wgpu::Extent3d {
                    width: self.config.width / 2, // Half resolution for performance
                    height: self.config.height / 2,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.render_target_view = self.render_target_texture.create_view(&wgpu::TextureViewDescriptor::default());
            
            // Update uniforms with new resolution
            let post_process_uniforms = PostProcessUniforms {
                time: self.camera_uniform.time[0], // Keep current time
                _pad1: 0.0,
                resolution: [self.config.width as f32, self.config.height as f32],
                _pad2: [0.0; 2],
            };
            self.queue.write_buffer(
                &self.post_process_uniform_buffer,
                0,
                bytemuck::cast_slice(&[post_process_uniforms]),
            );
            
            // Recreate bind group with the new view
            self.post_process_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Post Process Bind Group"),
                layout: &self.post_process_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&self.render_target_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.screen_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.post_process_uniform_buffer.as_entire_binding(),
                    },
                ],
            });
        }
    }

    fn update(&mut self) {
        let speed = 0.08;
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

        // Update Camera Uniform
        // Apply vertical movement (space / shift could be added, but for now we clamp)
        self.pos.y = self.pos.y.clamp(-0.5, 8.5);
        
        self.camera_uniform.time[0] += 0.016; // Simulate roughly 60fps delta time

        self.camera_uniform.update_view(self.yaw, self.pitch, self.pos);
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&[self.camera_uniform]),
        );
        
        // Update Post Process Uniforms (Time)
        let post_process_uniforms = PostProcessUniforms {
            time: self.camera_uniform.time[0],
            _pad1: 0.0,
            resolution: [self.config.width as f32, self.config.height as f32],
            _pad2: [0.0; 2],
        };
        self.queue.write_buffer(
            &self.post_process_uniform_buffer,
            0,
            bytemuck::cast_slice(&[post_process_uniforms]),
        );
    }

    fn window_event(&mut self, event: &WindowEvent) -> bool {
        let response = self.egui_state.on_window_event(self.window, event);
        if response.consumed {
            return true;
        }

        // Initialize audio on first user input (bypasses browser autoplay restrictions in WASM)
        if self.audio_stream.is_none() {
            if let Ok((stream, stream_handle)) = OutputStream::try_default() {
                if let Ok(sink) = Sink::try_new(&stream_handle) {
                    let source = wind_audio::WindSource::new(44100);
                    sink.append(source);
                    self.audio_stream = Some((stream, sink));
                }
            }
        }

        match event {
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(keycode),
                        state,
                        ..
                    },
                ..
            } => {
                let is_pressed = *state == ElementState::Pressed;
                match keycode {
                    KeyCode::KeyW => { self.w_pressed = is_pressed; true }
                    KeyCode::KeyS => { self.s_pressed = is_pressed; true }
                    KeyCode::KeyA => { self.a_pressed = is_pressed; true }
                    KeyCode::KeyD => { self.d_pressed = is_pressed; true }
                    KeyCode::KeyQ => { self.q_pressed = is_pressed; true }
                    KeyCode::KeyE => { self.e_pressed = is_pressed; true }
                    _ => false,
                }
            }
            WindowEvent::MouseInput { state: element_state, button: MouseButton::Left, .. } => {
                self.is_dragging = *element_state == ElementState::Pressed;
                true
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x, position.y);
                if self.is_dragging {
                    let dx = x - self.last_mouse_pos.0;
                    let dy = y - self.last_mouse_pos.1;
                    self.yaw += dx as f32 * 0.01;
                    self.pitch -= dy as f32 * 0.01;
                    self.pitch = self.pitch.clamp(-1.5, 1.5);
                }
                self.last_mouse_pos = (x, y);
                true
            }
            _ => false,
        }
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        // --- Calculate FPS ---
        let now = Instant::now();
        let delta_t = now.duration_since(self.last_frame_time).as_secs_f32();
        self.last_frame_time = now;
        let current_fps = 1.0 / delta_t.max(0.0001);
        self.fps_history.push(current_fps);
        if self.fps_history.len() > 60 {
            self.fps_history.remove(0);
        }
        let avg_fps = self.fps_history.iter().sum::<f32>() / self.fps_history.len() as f32;

        // --- Egui Update ---
        let raw_input = self.egui_state.take_egui_input(self.window);
        self.egui_ctx.begin_frame(raw_input);

        let mut show_panel = self.show_perf_panel;
        
        // Small floating toggle button
        egui::Area::new("PerfToggleArea".into())
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-10.0, -10.0))
            .show(&self.egui_ctx, |ui| {
                if ui.button("📊").clicked() {
                    show_panel = !show_panel;
                }
            });

        if show_panel {
            egui::Window::new("Performance")
                .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-10.0, -45.0))
                .collapsible(false)
                .resizable(false)
                .title_bar(false)
                .show(&self.egui_ctx, |ui| {
                    ui.label(format!("GPU: {}", self.gpu_name));
                    ui.label(format!("FPS: {:.1}", avg_fps));
                });
        }
        self.show_perf_panel = show_panel;

        let full_output = self.egui_ctx.end_frame();
        self.egui_state.handle_platform_output(self.window, full_output.platform_output);
        let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, self.egui_ctx.pixels_per_point());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        // Update Egui textures and buffers
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: self.egui_ctx.pixels_per_point(),
        };

        for (id, image_delta) in &full_output.textures_delta.set {
            self.egui_renderer.update_texture(&self.device, &self.queue, *id, image_delta);
        }
        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );

        // Pass 1: Render scene to the render target texture
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass 1 (Scene)"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.render_target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.render_bind_group, &[]);
            render_pass.draw(0..3, 0..1);
        }

        // Pass 2: Apply post-processing and render to screen
        {
            let mut post_process_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass 2 (Post Process)"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            post_process_pass.set_pipeline(&self.post_process_pipeline);
            post_process_pass.set_bind_group(0, &self.post_process_bind_group, &[]);
            post_process_pass.draw(0..3, 0..1);

            // Render egui UI on top of everything
            self.egui_renderer.render(&mut post_process_pass, &paint_jobs, &screen_descriptor);
        }

        // Free textures
        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub async fn run() {
    #[cfg(target_arch = "wasm32")]
    {
        std::panic::set_hook(Box::new(console_error_panic_hook::hook));
        console_log::init_with_level(log::Level::Warn).expect("Couldn't initialize logger");
    }

    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_title("Nubes - Volumetric Raymarching")
        .build(&event_loop)
        .unwrap();

    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::WindowExtWebSys;
        web_sys::window()
            .and_then(|win| win.document())
            .and_then(|doc| {
                let dst = doc.get_element_by_id("wasm-example")?;
                let canvas = web_sys::Element::from(window.canvas().unwrap());
                dst.append_child(&canvas).ok()?;
                Some(())
            })
            .expect("Couldn't append canvas to document body.");
    }

    let mut state = State::new(&window).await;

    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent {
                ref event,
                window_id,
            } if window_id == state.window.id() => {
                if !state.window_event(event) {
                    match event {
                        WindowEvent::CloseRequested
                        | WindowEvent::KeyboardInput {
                            event:
                                KeyEvent {
                                    state: ElementState::Pressed,
                                    physical_key: PhysicalKey::Code(KeyCode::Escape),
                                    ..
                                },
                            ..
                        } => elwt.exit(),
                        WindowEvent::Resized(physical_size) => {
                            state.resize(*physical_size);
                        }
                        WindowEvent::RedrawRequested => {
                            state.update();
                            match state.render() {
                                Ok(_) => {}
                                Err(wgpu::SurfaceError::Lost) => state.resize(state.size),
                                Err(wgpu::SurfaceError::OutOfMemory) => elwt.exit(),
                                Err(e) => eprintln!("{:?}", e),
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::AboutToWait => {
                state.window.request_redraw();
            }
            _ => {}
        }
    }).unwrap();
}
