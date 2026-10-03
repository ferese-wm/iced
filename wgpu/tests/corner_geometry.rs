use iced_wgpu::core::shape::{self, Outline, Shape, edge_coverage};
use wgpu::util::DeviceExt;
#[test]
#[ignore = "requires a GPU or software Vulkan adapter"]
fn gpu_distance_matches_cpu_profile() {
    futures::executor::block_on(run());
}
async fn run() {
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .expect("GPU adapter");
    println!("Adapter: {:?}", adapter.get_info());
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .unwrap();
    let mut data = Vec::<u8>::new();
    let mut expected = Vec::<f64>::new();
    for shape in [Shape::Circular, Shape::Continuous] {
        for radii in [
            [0.; 4],
            [0.375; 4],
            [8.25; 4],
            [24.; 4],
            [29.5; 4],
            [0., 29.5, 3., 18.],
            [28., 2., 21., 0.],
        ] {
            for scale in [1.0f32, 1.25, 1.5] {
                let bounds =
                    [1.25 * scale, 2.5 * scale, 61.5 * scale, 59. * scale];
                let r = radii.map(|r| r * scale);
                for inset in [0.0f32, 0.75, 12., 32.] {
                    let outline = Outline::new(
                        bounds.map(f64::from),
                        r.map(f64::from),
                        shape,
                    )
                    .unwrap()
                    .inset(f64::from(inset))
                    .unwrap();
                    for y in (0..95).step_by(3) {
                        for x in (0..98).step_by(3) {
                            let p = [x as f32 + 0.5, y as f32 + 0.5];
                            for v in [p[0], p[1], 0., 0.]
                                .into_iter()
                                .chain(bounds)
                                .chain(r)
                            {
                                data.extend(v.to_le_bytes());
                            }
                            for v in [shape as u32, inset.to_bits(), 0, 0] {
                                data.extend(v.to_le_bytes());
                            }
                            expected.push(
                                outline.signed_distance(p.map(f64::from)),
                            );
                        }
                    }
                }
            }
        }
    }
    let source = format!(
        "{}\n{}",
        shape::WGSL,
        r#"
struct Query { p: vec4<f32>, bounds: vec4<f32>, radii: vec4<f32>, style: vec4<u32> }
@group(0) @binding(0) var<storage, read> queries: array<Query>;
@group(0) @binding(1) var<storage, read_write> results: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= arrayLength(&queries) { return; }
    let q = queries[id.x];
    results[id.x] = shape_distance(q.p.xy, q.bounds, q.radii, q.style.x, bitcast<f32>(q.style.y));
}
"#
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shape reference"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline =
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &data,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let size = (expected.len() * 4) as u64;
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: input.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass =
            encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups((expected.len() as u32).div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, size);
    let submission = queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    let _ = device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: None,
        })
        .unwrap();
    rx.recv().unwrap().unwrap();
    let mapped = readback.slice(..).get_mapped_range();
    let mut max_error = 0.0f64;
    for (i, (bytes, expected)) in
        mapped.chunks_exact(4).zip(expected.iter()).enumerate()
    {
        let actual = f64::from(f32::from_le_bytes(bytes.try_into().unwrap()));
        max_error = max_error.max((actual - expected).abs());
        assert!(
            (actual - expected).abs() < 0.01,
            "sample {i}: GPU {actual}, CPU {expected}"
        );
        assert!(
            (edge_coverage(actual) - edge_coverage(*expected)).abs()
                <= 2. / 255.,
            "coverage {i}"
        );
    }
    println!(
        "{} samples passed, max distance error {max_error}",
        expected.len()
    );
}
