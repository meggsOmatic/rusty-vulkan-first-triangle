use crate::config::*;
use crate::device::*;
use crate::swapsurface::*;
use crate::window::*;

use anyhow::{Context, Result};

use ash::prelude::*;
use ash::vk;
use safe_transmute::*;
use std::default::Default;
use std::ffi::CStr;
use std::rc::Rc;


static VERTEX_BYTECODE: &'static [u8] = include_bytes!("./vert.spv");
static FRAGMENT_BYTECODE: &'static [u8] = include_bytes!("./frag.spv");

pub struct Renderer {
    pub device: Rc<Device>,
    pub renderpass: vk::RenderPass,
    pub vertex_shader_module: vk::ShaderModule,
    pub fragment_shader_module: vk::ShaderModule,
    pub pipeline_layout: vk::PipelineLayout,
    pub pipeline: vk::Pipeline,
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

        let pipeline_layout = device
            .device
            .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default(), None)
            .context("Could not create pipeline layout")?;

        let create_shader_module = |bytecode| {
            let code = transmute_many::<u32, PedanticGuard>(bytecode).unwrap();
            let shadermodule_info = vk::ShaderModuleCreateInfo::default().code(code);
            device.device.create_shader_module(&shadermodule_info, None)
        };
        let vertex_shader_module = create_shader_module(&VERTEX_BYTECODE).context("Could not create vertex bytecode")?;
        let fragment_shader_module = create_shader_module(&FRAGMENT_BYTECODE).context("Could not create fragment bytecode")?;

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

        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();

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

        Ok(Renderer {
            device,
            renderpass,
            vertex_shader_module,
            fragment_shader_module,
            pipeline_layout,
            pipeline,
        })
    }

    pub unsafe fn render(&mut self, win: &mut VulkanWindow) -> VkResult<()> {
        let dev: &ash::Device = &self.device.device;

        let pf = &win.per_frame[win.frame_count % win.per_frame.len()];

        dev.wait_for_fences(&[pf.in_flight_fence], true, u64::max_value())?;

        win.frame_count += 1;
        let now = std::time::Instant::now();
        let elapsed = (now - win.count_start_time).as_secs_f64();
        if elapsed > 1.0 {
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
                        float32: [1.0, 1.0, 1.0, 0.0],
                    },
                }]),
            vk::SubpassContents::INLINE,
        );

        dev.cmd_bind_pipeline(
            pf.command_buffer,
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline,
        );

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

        Ok(())
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
        }
    }
}
