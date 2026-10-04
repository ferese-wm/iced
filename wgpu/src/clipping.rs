use crate::core::{Size, shape};
use crate::graphics::layer::ShapedClip;
use bytemuck::{Pod, Zeroable};
use std::{cell::RefCell, collections::VecDeque};
use wgpu::util::DeviceExt;

const MASK_CACHE_BYTES: u64 = 32 * 1024 * 1024;
const COLOR_POOL_BYTES: u64 = 32 * 1024 * 1024;

struct CachedMask {
    clips: Vec<ShapedClip>,
    scale: f32,
    view: wgpu::TextureView,
}

#[derive(Clone)]
pub(crate) struct Target {
    pub color: wgpu::TextureView,
    pub mask: RefCell<Option<wgpu::TextureView>>,
}

pub(crate) struct Pipeline {
    mask_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    targets: Vec<Target>,
    size: Size<u32>,
    masks: VecDeque<CachedMask>,
    color_bytes: u64,
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
            wgpu::TextureFormat::R8Unorm,
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
            masks: VecDeque::new(),
            color_bytes: 0,
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
            self.masks.clear();
            self.size = size;
        }

        while self.targets.len() < depth {
            let color = texture(device, format, size);
            self.targets.push(Target {
                color,
                mask: RefCell::new(None),
            });
        }

        self.color_bytes = u64::from(size.width)
            * u64::from(size.height)
            * u64::from(format.block_copy_size(None).unwrap_or(16));
        self.targets[..depth].to_vec()
    }

    pub(crate) fn begin(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &Target,
        clips: &[ShapedClip],
        scale: f32,
    ) {
        let _ = begin_pass(
            encoder,
            &target.color,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        // Group IDs identify painter-order scopes, not mask geometry. Keep
        // sibling masks in an LRU instead of overwriting one slot per depth.
        let clips: Vec<_> = clips
            .iter()
            .cloned()
            .map(|mut clip| {
                clip.id = 0;
                clip
            })
            .collect();
        if let Some(index) = self
            .masks
            .iter()
            .position(|mask| mask.clips == clips && mask.scale == scale)
        {
            let mask = self.masks.remove(index).expect("cached clip mask");
            *target.mask.borrow_mut() = Some(mask.view.clone());
            self.masks.push_front(mask);
            return;
        }
        let mask = texture(device, wgpu::TextureFormat::R8Unorm, self.size);
        *target.mask.borrow_mut() = Some(mask.clone());
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
                &mask,
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
        let bytes = u64::from(self.size.width) * u64::from(self.size.height);
        if bytes > 0 && bytes <= MASK_CACHE_BYTES {
            while (self.masks.len() as u64 + 1) * bytes > MASK_CACHE_BYTES
                || self.masks.len() >= 32
            {
                let _ = self.masks.pop_back();
            }
            self.masks.push_front(CachedMask {
                clips,
                scale,
                view: mask,
            });
        }
    }

    pub(crate) fn trim(&mut self, depth: usize) {
        // Active command buffers own their texture references. Bound only the
        // reusable pool; deep frames must not retain their peak allocations.
        let retained = if self.color_bytes == 0 {
            0
        } else {
            (COLOR_POOL_BYTES / self.color_bytes) as usize
        };
        self.targets.truncate(depth.min(retained));
    }

    pub(crate) fn composite(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &Target,
        parent: Option<&Target>,
        destination: &wgpu::TextureView,
    ) {
        let source_mask = source.mask.borrow();
        let source_mask = source_mask.as_ref().expect("begun group mask");
        let parent_mask = parent.map(|parent| parent.mask.borrow());
        let parent_mask = parent_mask
            .as_ref()
            .and_then(|mask| mask.as_ref())
            .unwrap_or(source_mask);
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
                    resource: wgpu::BindingResource::TextureView(source_mask),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(parent_mask),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Rectangle;

    #[test]
    #[ignore = "requires a GPU or software Vulkan adapter"]
    fn sibling_masks_reuse_geometry_and_retained_targets_obey_budget() {
        futures::executor::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions::default())
                .await
                .unwrap();
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor::default())
                .await
                .unwrap();
            let format = wgpu::TextureFormat::Rgba8Unorm;
            let mut pipeline = Pipeline::new(&device, format);
            let size = Size::new(1024, 1024);
            let targets = pipeline.targets(&device, format, size, 12);
            let mut encoder =
                device.create_command_encoder(&Default::default());
            let clip = |x, id| ShapedClip {
                id,
                local_border: None,
                bounds: Rectangle::INFINITE,
                outline: shape::Outline::new(
                    [x, 10.0, 100.0, 60.0],
                    [12.0; 4],
                    shape::Shape::Continuous,
                ),
            };
            pipeline.begin(
                &device,
                &mut encoder,
                &targets[0],
                &[clip(10.0, 1)],
                1.0,
            );
            let first = targets[0].mask.borrow().clone().unwrap();
            pipeline.begin(
                &device,
                &mut encoder,
                &targets[0],
                &[clip(200.0, 2)],
                1.0,
            );
            let sibling = targets[0].mask.borrow().clone().unwrap();
            assert_ne!(first, sibling);
            pipeline.begin(
                &device,
                &mut encoder,
                &targets[0],
                &[clip(10.0, 99)],
                1.0,
            );
            assert_eq!(targets[0].mask.borrow().as_ref(), Some(&first));
            pipeline.begin(
                &device,
                &mut encoder,
                &targets[0],
                &[clip(200.0, 100)],
                1.0,
            );
            assert_eq!(targets[0].mask.borrow().as_ref(), Some(&sibling));
            assert_eq!(pipeline.masks.len(), 2);
            pipeline.begin(
                &device,
                &mut encoder,
                &targets[0],
                &[clip(10.0, 1)],
                1.25,
            );
            assert_ne!(targets[0].mask.borrow().as_ref(), Some(&first));
            for i in 0..40 {
                pipeline.begin(
                    &device,
                    &mut encoder,
                    &targets[0],
                    &[clip(i as f64, i)],
                    1.0,
                );
            }
            assert!(
                pipeline.masks.len() as u64 * 1024 * 1024 <= MASK_CACHE_BYTES
            );
            pipeline.trim(12);
            assert_eq!(pipeline.targets.len(), 8);
            assert!(
                pipeline.targets.len() as u64 * pipeline.color_bytes
                    <= COLOR_POOL_BYTES
            );
            let _ = queue.submit([encoder.finish()]);
            drop(targets);
            let _ = pipeline.targets(&device, format, Size::new(100, 100), 1);
            assert!(pipeline.masks.is_empty());
            pipeline.trim(0);
            assert!(pipeline.targets.is_empty());
        });
    }
}
