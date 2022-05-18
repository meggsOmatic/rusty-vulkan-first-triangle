use crate::config::*;
use crate::device::*;
use crate::swapsurface::*;
use crate::util::as_byte_slice;
use crate::window::*;

use anyhow::{Context, Result};

use anyhow::*;
use ash::prelude::*;
use ash::vk;
use glam::*;
use safe_transmute::*;
use std::default::Default;
use std::ffi::CStr;
use std::mem;
use std::rc::Rc;

static VERTEX_BYTECODE: &'static [u8] = include_bytes!("./vert.spv");
static FRAGMENT_BYTECODE: &'static [u8] = include_bytes!("./frag.spv");

#[repr(C, packed)]
pub struct Vertex {
    pub pos: Vec2,
    pub color: Vec3,
}

impl Vertex {
    fn get_description() -> (
        vk::VertexInputBindingDescription,
        Vec<vk::VertexInputAttributeDescription>,
    ) {
        (
            vk::VertexInputBindingDescription {
                binding: 0,
                stride: mem::size_of::<Vertex>() as u32,
                input_rate: vk::VertexInputRate::VERTEX,
            },
            vec![
                vk::VertexInputAttributeDescription {
                    binding: 0,
                    location: 0,
                    format: vk::Format::R32G32_SFLOAT,
                    offset: memoffset::offset_of!(Vertex, pos) as u32,
                },
                vk::VertexInputAttributeDescription {
                    binding: 0,
                    location: 1,
                    format: vk::Format::R32G32B32_SFLOAT,
                    offset: memoffset::offset_of!(Vertex, color) as u32,
                },
            ],
        )
    }
}

static TRIANGLE: &'static [Vertex] = &[
    Vertex {
        pos: const_vec2!([0.0, -0.5]),
        color: const_vec3!([1.0, 0.0, 0.0]),
    },
    Vertex {
        pos: const_vec2!([0.5, 0.5]),
        color: const_vec3!([0.0, 1.0, 0.0]),
    },
    Vertex {
        pos: const_vec2!([-0.5, 0.5]),
        color: const_vec3!([0.0, 0.0, 1.0]),
    },
];

pub struct Renderer {
    pub device: Rc<Device>,
    pub renderpass: vk::RenderPass,
    pub vertex_shader_module: vk::ShaderModule,
    pub fragment_shader_module: vk::ShaderModule,
    pub pipeline_layout: vk::PipelineLayout,
    pub pipeline: vk::Pipeline,
    pub vertex_buffer: vk::Buffer,
    pub vertex_buffer_memory: vk::DeviceMemory,
    pub start_time: std::time::SystemTime
}

impl Renderer {
    pub unsafe fn new(device: Rc<Device>, swap: &PerSwapchain) -> Result<Self> {
        let color_attachment_desc = [vk::AttachmentDescription::default()
            .format(swap.format.format)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)];

        let color_attchment_ref = [vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

        let subpass = [vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&color_attchment_ref)];

        let subpass_dependencies = [vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .src_access_mask(vk::AccessFlags::empty())
            .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)];

        let renderpass_info = vk::RenderPassCreateInfo::default()
            .attachments(&color_attachment_desc)
            .subpasses(&subpass)
            .dependencies(&subpass_dependencies);

        let renderpass = device.device.create_render_pass(&renderpass_info, None)?;

        let push_constant_ranges = [vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::FRAGMENT,
            size: 16,
            offset: 0
        }];
        let pipeline_layout = device
            .device
            .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default()
                .push_constant_ranges(&push_constant_ranges), None)
            .context("Could not create pipeline layout")?;

        let create_shader_module = |bytecode| {
            let code = transmute_many::<u32, PedanticGuard>(bytecode).unwrap();
            let shadermodule_info = vk::ShaderModuleCreateInfo::default().code(code);
            device.device.create_shader_module(&shadermodule_info, None)
        };
        let vertex_shader_module =
            create_shader_module(&VERTEX_BYTECODE).context("Could not create vertex bytecode")?;
        let fragment_shader_module = create_shader_module(&FRAGMENT_BYTECODE)
            .context("Could not create fragment bytecode")?;

        let create_shader_stage = |module, stage| {
            vk::PipelineShaderStageCreateInfo::default()
                .module(module)
                .stage(stage)
                .name(CStr::from_bytes_with_nul(b"main\0").unwrap())
        };

        let shader_stages = [
            create_shader_stage(vertex_shader_module, vk::ShaderStageFlags::VERTEX),
            create_shader_stage(fragment_shader_module, vk::ShaderStageFlags::FRAGMENT),
        ];

        let vertex_desc = Vertex::get_description();
        let input_descs = [vertex_desc.0];
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&input_descs)
            .vertex_attribute_descriptions(&vertex_desc.1);

        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);

        let viewport = [vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: swap.size.width as f32,
            height: swap.size.height as f32,
            min_depth: 0.0,
            max_depth: 1.,
        }];

        let scissor = [vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: swap.size,
        }];

        let viewport_info = vk::PipelineViewportStateCreateInfo::default()
            .viewports(&viewport)
            .scissors(&scissor);

        let rasterizer_info = vk::PipelineRasterizationStateCreateInfo::default()
            .depth_clamp_enable(false)
            .rasterizer_discard_enable(false)
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::CLOCKWISE);

        let multisample_info = vk::PipelineMultisampleStateCreateInfo::default()
            .sample_shading_enable(false)
            .min_sample_shading(1.0)
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);

        let blendattachment_info = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(
                vk::ColorComponentFlags::R
                    | vk::ColorComponentFlags::G
                    | vk::ColorComponentFlags::B
                    | vk::ColorComponentFlags::A,
            )
            .blend_enable(false)];

        let colorblend_info =
            vk::PipelineColorBlendStateCreateInfo::default().attachments(&blendattachment_info);

        let mut dyn_states = Vec::<vk::DynamicState>::new();
        if VK_DYNAMIC_VIEW_SIZE {
            dyn_states.push(vk::DynamicState::VIEWPORT);
            dyn_states.push(vk::DynamicState::SCISSOR);
        }
        let dyn_state = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dyn_states);

        let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_info)
            .rasterization_state(&rasterizer_info)
            .multisample_state(&multisample_info)
            .color_blend_state(&colorblend_info)
            .layout(pipeline_layout)
            .render_pass(renderpass)
            .dynamic_state(&dyn_state)
            .subpass(0);

        let pipeline = device
            .device
            .create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
            .unwrap()[0];

        let vertex_buffer = device
            .device
            .create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(mem::size_of_val(TRIANGLE) as u64)
                    .usage(vk::BufferUsageFlags::VERTEX_BUFFER)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )
            .context("Creating vertex buffer")?;

        let mem_reqs = device.device.get_buffer_memory_requirements(vertex_buffer);
        dbg!(mem_reqs);
        let mem_props = device
            .loaders
            .instance
            .get_physical_device_memory_properties(device.physical_device);
        dbg!(mem_props);

        let type_index = (0..mem_props.memory_type_count)
            .find(|&type_index| {
                mem_reqs.memory_type_bits & (1 << type_index) != 0
                    && mem_props.memory_types[type_index as usize]
                        .property_flags
                        .contains(vk::MemoryPropertyFlags::HOST_VISIBLE)
            })
            .context("Could not find a memory type for the vertex buffer")?;

        let vertex_buffer_memory = device
            .device
            .allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(mem_reqs.size)
                    .memory_type_index(type_index),
                None,
            )
            .context("Could not allocate vertex buffer memory")?;

        device
            .device
            .bind_buffer_memory(vertex_buffer, vertex_buffer_memory, 0)
            .context("Binding vertex buffer memory")?;

        let map_ptr = device
            .device
            .map_memory(
                vertex_buffer_memory,
                0,
                vk::WHOLE_SIZE,
                vk::MemoryMapFlags::empty(),
            )
            .context("Mapping vertex buffer memory")?;

        std::ptr::copy_nonoverlapping(TRIANGLE.as_ptr(), map_ptr as *mut Vertex, TRIANGLE.len());

        device.device.flush_mapped_memory_ranges(&[vk::MappedMemoryRange {
            memory: vertex_buffer_memory,
            offset: 0,
            size: vk::WHOLE_SIZE,
            ..Default::default()
        }]).context("Flushing caches")?;

        device.device.unmap_memory(vertex_buffer_memory);

        Ok(Renderer {
            device,
            renderpass,
            vertex_shader_module,
            fragment_shader_module,
            pipeline_layout,
            pipeline,
            vertex_buffer,
            vertex_buffer_memory,
            start_time: std::time::SystemTime::now()
        })
    }

    pub unsafe fn render(&mut self, win: &mut VulkanWindow) -> VkResult<()> {
        let dev: &ash::Device = &self.device.device;

        let pf = &win.per_frame[win.frame_count % win.per_frame.len()];

        dev.wait_for_fences(&[pf.in_flight_fence], true, u64::max_value())?;

        win.frame_count += 1;
        let now = std::time::Instant::now();
        let elapsed = (now - win.count_start_time).as_secs_f64();
        if false && elapsed > 1.0 {
            let num_frames = win.frame_count - win.count_start_frame;
            println!(
                "{} frames in {:.3} secs, average time {:.2} msecs or {:.1} FPS",
                num_frames,
                elapsed,
                elapsed * 1000. / num_frames as f64,
                num_frames as f64 / elapsed
            );
            win.count_start_frame = win.frame_count;
            win.count_start_time = now;
        }

        let (swap_index, _) = self.device.swapchain_loader.acquire_next_image(
            win.swap.swapchain,
            u64::MAX,
            pf.image_available_semaphore,
            vk::Fence::null(),
        )?;

        dev.reset_command_buffer(pf.command_buffer, vk::CommandBufferResetFlags::empty())?;

        dev.begin_command_buffer(pf.command_buffer, &vk::CommandBufferBeginInfo::default())?;
        dev.cmd_begin_render_pass(
            pf.command_buffer,
            &vk::RenderPassBeginInfo::default()
                .render_pass(self.renderpass)
                .framebuffer(win.swap.framebuffers[swap_index as usize])
                .render_area(win.swap.size.into())
                .clear_values(&[vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: win.background_color
                    },
                }]),
            vk::SubpassContents::INLINE,
        );

        dev.cmd_bind_pipeline(
            pf.command_buffer,
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline,
        );

        dev.cmd_bind_vertex_buffers(pf.command_buffer, 0, &[self.vertex_buffer], &[0]);

        if VK_DYNAMIC_VIEW_SIZE {
            dev.cmd_set_viewport(
                pf.command_buffer,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: win.swap.size.width as f32,
                    height: win.swap.size.height as f32,
                    min_depth: 0.0,
                    max_depth: 1.,
                }],
            );

            dev.cmd_set_scissor(
                pf.command_buffer,
                0,
                &[vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: win.swap.size,
                }],
            );
        }

        let time = std::time::Instant::now().duration_since(win.anim_start_time).as_secs_f64() as f32;
        let min_dim = std::cmp::min(win.swap.size.width, win.swap.size.height) as f32;
        let pcs = vec4(
            min_dim / win.swap.size.width as f32,
            min_dim / win.swap.size.height as f32,
            time * win.shape_rotate_speed,
            time * win.color_rotate_speed
        );
        
        dev.cmd_push_constants(pf.command_buffer, self.pipeline_layout, vk::ShaderStageFlags::FRAGMENT, 0, as_byte_slice(&pcs));
        dev.cmd_draw(pf.command_buffer, 3, 1, 0, 0);
        dev.cmd_end_render_pass(pf.command_buffer);
        dev.end_command_buffer(pf.command_buffer)?;

        dev.reset_fences(&[pf.in_flight_fence])?;
        dev.queue_submit(
            self.device.graphics_queue,
            &[vk::SubmitInfo::default()
                .wait_semaphores(&[pf.image_available_semaphore])
                .wait_dst_stage_mask(&[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT])
                .command_buffers(&[pf.command_buffer])
                .signal_semaphores(&[pf.render_finished_semaphore])],
            pf.in_flight_fence,
        )?;

        self.device.swapchain_loader.queue_present(
            self.device.present_queue,
            &vk::PresentInfoKHR::default()
                .wait_semaphores(&[pf.render_finished_semaphore])
                .swapchains(&[win.swap.swapchain])
                .image_indices(&[swap_index]),
        )?;

        Result::Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device.device_wait_idle();
            self.device.device.destroy_pipeline(self.pipeline, None);
            self.device
                .device
                .destroy_render_pass(self.renderpass, None);
            self.device
                .device
                .destroy_pipeline_layout(self.pipeline_layout, None);
            self.device
                .device
                .destroy_shader_module(self.fragment_shader_module, None);
            self.device
                .device
                .destroy_shader_module(self.vertex_shader_module, None);
            self.device.device.destroy_buffer(self.vertex_buffer, None);
            self.device
                .device
                .free_memory(self.vertex_buffer_memory, None);
        }
    }
}
