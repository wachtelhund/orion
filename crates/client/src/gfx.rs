//! wgpu renderer: one pipeline, one procedural sprite atlas, one instanced
//! draw call per frame. Every visible thing — terrain, sprites, tracers,
//! text — is an atlas-sampled (optionally rotated) quad instance.

use std::sync::Arc;

use winit::window::Window;

use crate::atlas::{self, Region, SpriteBook};
use crate::font;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Inst {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub color: [f32; 4],
    /// Rotation around the quad center, radians. 0 = axis-aligned.
    pub rot: [f32; 2], // [rot, unused] — keeps 8-byte stride alignment
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    screen: [f32; 2],
    _pad: [f32; 2],
}

pub struct Gfx {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    pipeline_add: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    inst_buf: wgpu::Buffer,
    inst_cap: usize,
    glow_buf: wgpu::Buffer,
    glow_cap: usize,
    pub book: SpriteBook,
    /// Format frames are rendered through — the sRGB variant of the
    /// surface format when the surface itself can't be sRGB (WebGPU).
    view_format: wgpu::TextureFormat,
    /// When set, the next rendered frame is written to this path as PPM.
    pub capture: Option<String>,
}

impl Gfx {
    pub fn new(window: Arc<Window>) -> Gfx {
        pollster::block_on(Self::new_async(window))
    }

    pub async fn new_async(window: Arc<Window>) -> Gfx {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window).expect("create surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("no gpu adapter");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await
            .expect("request device");

        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("surface config");
        // The shader writes linear values and relies on an sRGB surface to
        // encode them. Browsers hand out the non-sRGB variant first, which
        // displayed linear values raw — the whole game rendered too dark.
        let caps = surface.get_capabilities(&adapter);
        let srgb = config.format.add_srgb_suffix();
        let view_format = if caps.formats.contains(&srgb) {
            // Straightforward: an sRGB swapchain format exists (native).
            config.format = srgb;
            srgb
        } else if config.format != srgb {
            // WebGPU canvases only accept non-sRGB formats directly; the
            // sRGB encode goes through a view format instead. Without this
            // the shader's linear output displayed raw — far too dark.
            config.view_formats = vec![srgb];
            srgb
        } else {
            config.format
        };
        #[cfg(target_arch = "wasm32")]
        crate::weblog(&format!(
            "orion: surface {:?} via view {:?}",
            config.format, view_format
        ));
        config.present_mode = wgpu::PresentMode::AutoVsync;
        config.usage |= wgpu::TextureUsages::COPY_SRC; // frame capture
        surface.configure(&device, &config);

        // ---- atlas ----
        let (pixels, book) = atlas::build();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d {
                width: atlas::ATLAS,
                height: atlas::ATLAS,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // sRGB: atlas colors are authored in sRGB; sampling converts to
            // linear, the sRGB surface converts back — round-trip identity.
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(atlas::ATLAS * 4),
                rows_per_image: Some(atlas::ATLAS),
            },
            wgpu::Extent3d {
                width: atlas::ATLAS,
                height: atlas::ATLAS,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprite"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pl"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprites"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Inst>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2, 1 => Float32x2, 2 => Float32x2,
                        3 => Float32x2, 4 => Float32x4, 5 => Float32x2
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: view_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Additive variant: same shader, light accumulates instead of
        // blending — the emissive/glow pass.
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline_add = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprites-additive"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Inst>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2, 1 => Float32x2, 2 => Float32x2,
                        3 => Float32x2, 4 => Float32x4, 5 => Float32x2
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: view_format,
                    blend: Some(additive),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let inst_cap = 1 << 15;
        let inst_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: (inst_cap * std::mem::size_of::<Inst>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let glow_cap = 1 << 12;
        let glow_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glow-instances"),
            size: (glow_cap * std::mem::size_of::<Inst>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Gfx {
            surface,
            device,
            queue,
            config,
            pipeline,
            pipeline_add,
            globals_buf,
            bind_group,
            inst_buf,
            inst_cap,
            glow_buf,
            glow_cap,
            book,
            view_format,
            capture: None,
        }
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
    }

    /// Draw world sprites (alpha), then the glow list (additive), then UI
    /// sprites (alpha) — `world_n` splits `instances` into world/UI halves
    /// so glow light lands under the console, not over it.
    pub fn render(&mut self, instances: &[Inst], world_n: usize, glows: &[Inst]) {
        if instances.len() > self.inst_cap {
            self.inst_cap = instances.len().next_power_of_two();
            self.inst_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instances"),
                size: (self.inst_cap * std::mem::size_of::<Inst>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if glows.len() > self.glow_cap {
            self.glow_cap = glows.len().next_power_of_two();
            self.glow_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("glow-instances"),
                size: (self.glow_cap * std::mem::size_of::<Inst>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        self.queue.write_buffer(&self.inst_buf, 0, bytemuck::cast_slice(instances));
        if !glows.is_empty() {
            self.queue.write_buffer(&self.glow_buf, 0, bytemuck::cast_slice(glows));
        }
        let globals = Globals {
            screen: [self.config.width as f32, self.config.height as f32],
            _pad: [0.0; 2],
        };
        self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));

        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(_) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.view_format),
            ..Default::default()
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.016,
                            g: 0.014,
                            b: 0.02,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.inst_buf.slice(..));
            let world_n = world_n.min(instances.len()) as u32;
            pass.draw(0..4, 0..world_n);
            if !glows.is_empty() {
                pass.set_pipeline(&self.pipeline_add);
                pass.set_vertex_buffer(0, self.glow_buf.slice(..));
                pass.draw(0..4, 0..glows.len() as u32);
                pass.set_pipeline(&self.pipeline);
                pass.set_vertex_buffer(0, self.inst_buf.slice(..));
            }
            pass.draw(0..4, world_n..instances.len() as u32);
        }
        self.queue.submit([encoder.finish()]);
        if let Some(path) = self.capture.take() {
            self.readback(&frame.texture, &path);
        }
        frame.present();
    }

    /// Copy the just-rendered frame to a PPM file (dev tooling: lets the
    /// build be verified visually without a human at the screen).
    fn readback(&self, texture: &wgpu::Texture, path: &str) {
        let w = self.config.width;
        let h = self.config.height;
        let bpr = (w * 4 + 255) / 256 * 256; // COPY_BYTES_PER_ROW_ALIGNMENT
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (bpr * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bpr),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::Maintain::Wait);
        let data = slice.get_mapped_range();
        let bgra = matches!(
            self.config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut out = Vec::with_capacity((w * h * 3) as usize + 32);
        out.extend_from_slice(format!("P6\n{w} {h}\n255\n").as_bytes());
        for y in 0..h {
            let row = &data[(y * bpr) as usize..];
            for x in 0..w as usize {
                let p = &row[x * 4..x * 4 + 4];
                if bgra {
                    out.extend_from_slice(&[p[2], p[1], p[0]]);
                } else {
                    out.extend_from_slice(&[p[0], p[1], p[2]]);
                }
            }
        }
        std::fs::write(path, out).expect("write capture");
        println!("captured frame -> {path}");
    }

    // ---- draw helpers (append instances) ----

    pub fn quad(&self, out: &mut Vec<Inst>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        let r = self.book.white;
        out.push(Inst {
            pos: [x, y],
            size: [w, h],
            uv_min: [r.u0, r.v0],
            uv_max: [r.u1, r.v1],
            color,
            rot: [0.0; 2],
        });
    }

    /// Sprite centered at (cx, cy).
    pub fn sprite(
        &self,
        out: &mut Vec<Inst>,
        r: Region,
        cx: f32,
        cy: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
    ) {
        out.push(Inst {
            pos: [cx - w * 0.5, cy - h * 0.5],
            size: [w, h],
            uv_min: [r.u0, r.v0],
            uv_max: [r.u1, r.v1],
            color,
            rot: [0.0; 2],
        });
    }

    /// Rotated sprite centered at (cx, cy).
    pub fn sprite_rot(
        &self,
        out: &mut Vec<Inst>,
        r: Region,
        cx: f32,
        cy: f32,
        w: f32,
        h: f32,
        rot: f32,
        color: [f32; 4],
    ) {
        out.push(Inst {
            pos: [cx - w * 0.5, cy - h * 0.5],
            size: [w, h],
            uv_min: [r.u0, r.v0],
            uv_max: [r.u1, r.v1],
            color,
            rot: [rot, 0.0],
        });
    }

    /// A line as a rotated thin quad — tracers, beams.
    pub fn beam(
        &self,
        out: &mut Vec<Inst>,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        thick: f32,
        color: [f32; 4],
    ) {
        let cx = (x0 + x1) * 0.5;
        let cy = (y0 + y1) * 0.5;
        let len = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
        let rot = (y1 - y0).atan2(x1 - x0);
        self.sprite_rot(out, self.book.white, cx, cy, len, thick, rot, color);
    }

    /// Draw text; returns pixel width. scale = pixel multiple of the 5x7 font.
    pub fn text(
        &self,
        out: &mut Vec<Inst>,
        x: f32,
        y: f32,
        scale: f32,
        color: [f32; 4],
        s: &str,
    ) -> f32 {
        let mut cx = x;
        for ch in s.chars() {
            if let Some(r) = self.book.glyph(ch) {
                // Glyph canvases carry a 1-texel outline border at their bake
                // scale; draw at the padded size, anchored a hair left/up, so
                // advance metrics stay (GLYPH_W + 1) while outlines meet.
                let pad = 1.0 / r.scale;
                out.push(Inst {
                    pos: [cx - pad * scale, y - pad * scale],
                    size: [
                        (font::GLYPH_W as f32 + 2.0 * pad) * scale,
                        (font::GLYPH_H as f32 + 2.0 * pad) * scale,
                    ],
                    uv_min: [r.u0, r.v0],
                    uv_max: [r.u1, r.v1],
                    color,
                    rot: [0.0; 2],
                });
            }
            cx += (font::GLYPH_W as f32 + 1.0) * scale;
        }
        cx - x
    }

    pub fn text_width(&self, scale: f32, s: &str) -> f32 {
        s.chars().count() as f32 * (font::GLYPH_W as f32 + 1.0) * scale
    }
}

const SHADER: &str = r#"
struct Globals {
    screen: vec2<f32>,
    _pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas_tex: texture_2d<f32>;
@group(0) @binding(2) var atlas_smp: sampler;

struct Inst {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) uv_min: vec2<f32>,
    @location(3) uv_max: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) rot: vec2<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, inst: Inst) -> VsOut {
    let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let center = inst.pos + inst.size * 0.5;
    let local = (corner - vec2<f32>(0.5, 0.5)) * inst.size;
    let cs = cos(inst.rot.x);
    let sn = sin(inst.rot.x);
    let rotated = vec2<f32>(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
    let p = center + rotated;
    let ndc = vec2<f32>(
        p.x / globals.screen.x * 2.0 - 1.0,
        1.0 - p.y / globals.screen.y * 2.0,
    );
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = mix(inst.uv_min, inst.uv_max, corner);
    // Instance colors are authored in sRGB; convert to linear so the sRGB
    // surface write round-trips them back to what was authored.
    out.color = vec4<f32>(pow(inst.color.rgb, vec3<f32>(2.2, 2.2, 2.2)), inst.color.a);
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let t = textureSample(atlas_tex, atlas_smp, in.uv);
    return t * in.color;
}
"#;
