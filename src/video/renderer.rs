use iced::{Rectangle, wgpu};

use crate::video::{Frame, PixelLayout};

pub struct FrameRenderer {
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    texture: Option<FrameTexture>,
}

struct FrameTexture {
    y: wgpu::Texture,
    uv: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
    layout: PixelLayout,
}

impl iced::widget::shader::Pipeline for FrameRenderer {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_WGSL.into()),
        });

        // A filterable float texture entry (used for both the luma and chroma
        // planes, at bindings 0 and 3).
        let plane_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video bind group layout"),
            entries: &[
                plane_entry(0), // luma (Y)
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
                plane_entry(3), // chroma (interleaved Cb,Cr)
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None, // video is opaque; just overwrite
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video uniforms"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            sampler,
            bind_group_layout,
            uniform_buffer,
            texture: None,
        }
    }
}

impl FrameRenderer {
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) {
        let size = (frame.width, frame.height);

        // The raw 16-bit P010 planes are uploaded reinterpreted as byte
        // channels (no CPU repacking): the shader reads the high byte. NV12 is
        // already 8-bit. All of these are core, filterable formats.
        let (y_format, uv_format) = match frame.layout {
            PixelLayout::P010 => (
                wgpu::TextureFormat::Rg8Unorm,
                wgpu::TextureFormat::Rgba8Unorm,
            ),
            PixelLayout::Nv12 => (wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rg8Unorm),
        };
        // Bytes per source row = frame width × bytes-per-sample (1 for NV12,
        // 2 for P010). The luma plane is `width` samples; the chroma plane is
        // also `width` samples (width/2 Cb,Cr pairs), so both share this.
        let row_bytes = frame.y.len() as u32 / frame.height;

        let stale = self.texture.as_ref().map(|t| (t.size, t.layout)) != Some((size, frame.layout));
        if stale {
            let y = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("video luma texture"),
                size: wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: y_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let uv = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("video chroma texture"),
                size: wgpu::Extent3d {
                    width: frame.width / 2,
                    height: frame.height / 2,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: uv_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let y_view = y.create_view(&wgpu::TextureViewDescriptor::default());
            let uv_view = uv.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("video bind group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&y_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&uv_view),
                    },
                ],
            });
            self.texture = Some(FrameTexture {
                y,
                uv,
                bind_group,
                size,
                layout: frame.layout,
            });
        }

        // Tell the shader which layout it's sampling (offset 8; the scale at
        // offset 0 is written by `update_uniforms`).
        let is_p010: u32 = matches!(frame.layout, PixelLayout::P010) as u32;
        queue.write_buffer(&self.uniform_buffer, 8, &is_p010.to_le_bytes());

        // Overwrite the plane textures' pixels in place.
        let tex = self.texture.as_ref().unwrap();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex.y,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.y,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(frame.height),
            },
            wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex.uv,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.uv,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes),
                rows_per_image: Some(frame.height / 2),
            },
            wgpu::Extent3d {
                width: frame.width / 2,
                height: frame.height / 2,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn update_uniforms(&self, queue: &wgpu::Queue, bounds: &Rectangle, vw: u32, vh: u32) {
        if bounds.height <= 0.0 || vh == 0 {
            return; // not laid out yet; avoid NaN
        }
        let widget_aspect = bounds.width / bounds.height;
        let video_aspect = vw as f32 / vh as f32;
        let (sx, sy) = if widget_aspect > video_aspect {
            (video_aspect / widget_aspect, 1.0)
        } else {
            (1.0, widget_aspect / video_aspect)
        };
        // Write only the scale (offset 0..8); the layout flag lives at offset 8.
        let mut bytes = [0u8; 8];
        bytes[0..4].copy_from_slice(&sx.to_le_bytes());
        bytes[4..8].copy_from_slice(&sy.to_le_bytes());
        queue.write_buffer(&self.uniform_buffer, 0, &bytes);
    }

    pub fn draw(&self, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(tex) = &self.texture else {
            return false; // no frame yet
        };
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &tex.bind_group, &[]);
        render_pass.draw(0..3, 0..1); // fullscreen triangle
        true
    }
}

const SHADER_WGSL: &str = r#"
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Uniforms {
    scale: vec2<f32>,
    is_p010: u32,
    _pad: u32,
};
@group(0) @binding(2) var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let uv = vec2<f32>(
        f32((vertex_index << 1u) & 2u),
        f32(vertex_index & 2u),
    );
    out.uv = uv;
    out.clip_position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    return out;
}

@group(0) @binding(0) var tex_y: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(3) var tex_uv: texture_2d<f32>;

// BT.709 limited-range (16-235 / 16-240) 8-bit YUV -> RGB.
fn yuv_to_rgb(y: f32, cb: f32, cr: f32) -> vec3<f32> {
    let yl = (y - 16.0 / 255.0) * (255.0 / 219.0);
    let u = (cb - 128.0 / 255.0) * (255.0 / 224.0);
    let v = (cr - 128.0 / 255.0) * (255.0 / 224.0);
    let r = yl + 1.5748 * v;
    let g = yl - 0.187324 * u - 0.468124 * v;
    let b = yl + 1.8556 * u;
    return clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Expand uv outward from the center by 1/scale, so the video occupies
    // only the `scale`-sized central region and the rest maps outside [0,1].
    let c = (in.uv - vec2<f32>(0.5)) / uniforms.scale + vec2<f32>(0.5);
    if (c.x < 0.0 || c.x > 1.0 || c.y < 0.0 || c.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0); // black bar
    }

    // Sample unconditionally (keeps texture access in uniform control flow),
    // then pick channels by layout. For P010 the value is the *high* byte:
    // luma .g, and chroma (Cb_lo,Cb_hi,Cr_lo,Cr_hi) -> .g and .a. NV12 is 8-bit
    // already: luma .r, chroma .r/.g.
    let yv = textureSample(tex_y, samp, c);
    let uvv = textureSample(tex_uv, samp, c);
    var y: f32;
    var cb: f32;
    var cr: f32;
    if (uniforms.is_p010 != 0u) {
        y = yv.g;
        cb = uvv.g;
        cr = uvv.a;
    } else {
        y = yv.r;
        cb = uvv.r;
        cr = uvv.g;
    }
    return vec4<f32>(yuv_to_rgb(y, cb, cr), 1.0);
}
"#;
