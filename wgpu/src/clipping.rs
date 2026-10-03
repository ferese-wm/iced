use crate::core::{Size, shape};
use crate::graphics::layer::ShapedClip;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

#[derive(Clone)]
pub(crate) struct Target {
    pub color: wgpu::TextureView,
    pub mask: wgpu::TextureView,
}

pub(crate) struct Pipeline {
    mask_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    targets: Vec<Target>,
    size: Size<u32>,
    mask_keys: Vec<Option<(Vec<ShapedClip>, f32)>>,
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct Contour {
    bounds: [f32; 4],
    radii: [f32; 4],
    clip: [f32; 4],
    style: [u32; 4],
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct Uniforms {
    count: [u32; 4],
    contours: [Contour; 16],
}

impl Pipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> Self {
        let mask_shader =
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("iced shaped clip mask"),
                source: wgpu::ShaderSource::Wgsl(
                    format!(
                        "{}\n{}",
                        shape::WGSL,
                        include_str!("shader/clip_mask.wgsl")
                    )
                    .into(),
                ),
            });
        let composite_shader =
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("iced shaped clip composite"),
                source: wgpu::ShaderSource::Wgsl(
                    include_str!("shader/clip_composite.wgsl").into(),
                ),
            });
        let blend = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Min,
        };
        let mask_pipeline = pipeline(
            device,
            &mask_shader,
            "mask_vs",
            "mask_fs",
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::BlendState {
                color: blend,
                alpha: blend,
            },
        );
        let composite_pipeline = pipeline(
            device,
            &composite_shader,
            "composite_vs",
            "composite_fs",
            format,
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );
        Self {
            mask_pipeline,
            composite_pipeline,
            targets: Vec::new(),
            size: Size::new(0, 0),
            mask_keys: Vec::new(),
        }
    }

    pub(crate) fn targets(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: Size<u32>,
        depth: usize,
    ) -> Vec<Target> {
        if self.size != size {
            self.targets.clear();
            self.mask_keys.clear();
            self.size = size;
        }

        while self.targets.len() < depth {
            let color = texture(device, format, size);
            let mask = texture(device, wgpu::TextureFormat::Rgba8Unorm, size);
            self.targets.push(Target { color, mask });
            self.mask_keys.push(None);
        }

        self.targets[..depth].to_vec()
    }

    pub(crate) fn begin(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &Target,
        depth: usize,
        clips: &[ShapedClip],
        scale: f32,
    ) {
        let _ = begin_pass(
            encoder,
            &target.color,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        if self.mask_keys[depth].as_ref().is_some_and(
            |(previous, previous_scale)| {
                previous == clips && *previous_scale == scale
            },
        ) {
            return;
        }

        self.mask_keys[depth] = Some((clips.to_vec(), scale));
        let mut first = true;

        for batch in clips.chunks(16) {
            let mut uniforms = Uniforms::zeroed();
            uniforms.count[0] = batch.len() as u32;

            for (output, clip) in uniforms.contours.iter_mut().zip(batch) {
                let Some((outline, bounds)) = clip.physical(scale, self.size)
                else {
                    continue;
                };
                *output = Contour {
                    bounds: outline.bounds().map(|v| v as f32),
                    radii: outline.radii().map(|v| v as f32),
                    clip: [bounds.x, bounds.y, bounds.width, bounds.height],
                    style: [
                        outline.shape() as u32,
                        (outline.inset_distance() as f32).to_bits(),
                        (bounds.width > 0.0 && bounds.height > 0.0) as u32,
                        0,
                    ],
                };
            }

            let buffer =
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("iced clip contours"),
                    contents: bytemuck::bytes_of(&uniforms),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iced clip contours"),
                layout: &self.mask_pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            let mut pass = begin_pass(
                encoder,
                &target.mask,
                if first {
                    wgpu::LoadOp::Clear(wgpu::Color::WHITE)
                } else {
                    wgpu::LoadOp::Load
                },
            );
            pass.set_pipeline(&self.mask_pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
            first = false;
        }
    }

    pub(crate) fn composite(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &Target,
        parent: Option<&Target>,
        destination: &wgpu::TextureView,
    ) {
        let nested = [parent.is_some() as u32, 0, 0, 0];
        let buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("iced clip nesting"),
                contents: bytemuck::bytes_of(&nested),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("iced clip content"),
            layout: &self.composite_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&source.color),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&source.mask),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &parent.unwrap_or(source).mask,
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });
        let mut pass = begin_pass(encoder, destination, wgpu::LoadOp::Load);
        pass.set_pipeline(&self.composite_pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

pub(crate) fn begin_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("iced clip pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

fn texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    size: Size<u32>,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("iced clipped group"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

fn pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    vertex: &str,
    fragment: &str,
    format: wgpu::TextureFormat,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("iced shaped clipping"),
        layout: None,
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vertex),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}
