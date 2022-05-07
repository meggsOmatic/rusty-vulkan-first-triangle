use crate::util::*;
use crate::config::*;
use crate::loaders::*;
use crate::device::*;
use crate::renderer::*;

use anyhow::{Context, Result};
use ash::extensions::ext::DebugUtils;
use ash::extensions::khr::{Swapchain, TimelineSemaphore};
use safe_transmute::guard::AllOrNothingGuard;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{Event, KeyboardInput, VirtualKeyCode, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::platform::windows::WindowExtWindows;
use winit::window::{Window, WindowBuilder};

use ash::prelude::*;
use ash::util::*;
use ash::vk::{self, SurfaceKHR, SwapchainKHR};
use safe_transmute::*;
use std::cmp;
use std::collections::HashMap;
use std::collections::{BTreeMap, HashSet};
use std::default::Default;
use std::ffi::CStr;
use std::ffi::CString;
use std::io::Cursor;
use std::mem;
use std::mem::align_of;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;


pub struct Surface {
    pub loaders: Rc<Loaders>,
    pub surface: vk::SurfaceKHR,
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            self.loaders.surface.destroy_surface(self.surface, None);
        }
    }
}

pub struct PerSwapchain {
    pub device: Rc<Device>,
    pub surface: Rc<Surface>,
    pub swapchain: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub views: Vec<vk::ImageView>,
    pub framebuffers: Vec<vk::Framebuffer>,
    pub size: vk::Extent2D,
    pub format: vk::SurfaceFormatKHR,
}

impl PerSwapchain {
    pub fn new(
        device: Rc<Device>,
        window: &Window,
        surface: Rc<Surface>,
        renderer: Option<&Renderer>,
        old: Option<&PerSwapchain>,
    ) -> Result<PerSwapchain> {
        unsafe {
            let capabilities = device
                .loaders
                .surface
                .get_physical_device_surface_capabilities(device.physical_device, surface.surface)
                .context("Could not get surface capabilities")?;

            let formats = device
                .loaders
                .surface
                .get_physical_device_surface_formats(device.physical_device, surface.surface)
                .context("Could not get surface formats")?;

            let present_modes = device
                .loaders
                .surface
                .get_physical_device_surface_present_modes(device.physical_device, surface.surface)
                .context("Could not get present modes")?;

            let image_count = capabilities.min_image_count;

            let window_size = window.inner_size();
            let swap_size = vk::Extent2D {
                width: cmp::min(
                    cmp::max(window_size.width, capabilities.min_image_extent.width),
                    capabilities.max_image_extent.width,
                ),
                height: cmp::min(
                    cmp::max(window_size.height, capabilities.min_image_extent.height),
                    capabilities.max_image_extent.height,
                ),
            };

            let format = (|| {
                let preference = [vk::SurfaceFormatKHR {
                    format: vk::Format::B8G8R8A8_SRGB,
                    color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
                }];

                for pref in preference {
                    if formats.contains(&pref) {
                        return pref;
                    }
                }
                formats[0]
            })();

            let present = (|| {
                let preference = [
                    vk::PresentModeKHR::MAILBOX,
                    vk::PresentModeKHR::IMMEDIATE,
                    vk::PresentModeKHR::FIFO_RELAXED,
                    vk::PresentModeKHR::FIFO,
                ];

                for pref in preference {
                    if present_modes.contains(&pref) {
                        return pref;
                    }
                }
                present_modes[0]
            })();

            let swapchain_info = vk::SwapchainCreateInfoKHR::default()
                .surface(surface.surface)
                .min_image_count(image_count)
                .image_color_space(format.color_space)
                .image_format(format.format)
                .image_extent(swap_size)
                .image_array_layers(1)
                .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
                .pre_transform(capabilities.current_transform)
                .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                .present_mode(present)
                .clipped(true)
                .old_swapchain(match old {
                    Some(swap) => swap.swapchain,
                    None => SwapchainKHR::default(),
                });

            let shared_queues = [device.present_queue_family, device.graphics_queue_family];
            let swapchain_info = if shared_queues[0] != shared_queues[1] {
                swapchain_info
                    .image_sharing_mode(vk::SharingMode::CONCURRENT)
                    .queue_family_indices(&shared_queues)
            } else {
                swapchain_info.image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            };

            let swapchain = device
                .swapchain_loader
                .create_swapchain(&swapchain_info, None)
                .context("Could not create swapchain")?;

            let images = device
                .swapchain_loader
                .get_swapchain_images(swapchain)
                .context("Could not get images for swapchain")?;

            let views = images
                .iter()
                .map(|&image| {
                    let subresource_info = vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .base_mip_level(0)
                        .level_count(1)
                        .base_array_layer(0)
                        .layer_count(1);

                    let view_info = vk::ImageViewCreateInfo::default()
                        .image(image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(format.format)
                        .components(vk::ComponentMapping::default())
                        .subresource_range(subresource_info);

                    device.device.create_image_view(&view_info, None)
                })
                .collect::<VkResult<Vec<vk::ImageView>>>()?;

            let mut result = PerSwapchain {
                device,
                surface,
                swapchain,
                images,
                views,
                framebuffers: Vec::new(),
                size: swap_size,
                format,
            };

            if let Some(r) = renderer {
                result
                    .create_framebuffers(r)
                    .context("Creating initial framebuffers")?;
            }

            Ok(result)
        }
    }

    pub fn create_framebuffers(&mut self, renderer: &Renderer) -> VkResult<()> {
        assert!(self.framebuffers.is_empty());
        assert!(!self.images.is_empty());
        assert_eq!(self.device.device.handle(), renderer.device.device.handle());

        unsafe {
            for &image_view in self.views.iter() {
                match self.device.device.create_framebuffer(
                    &vk::FramebufferCreateInfo::default()
                        .render_pass(renderer.renderpass)
                        .attachments(&[image_view])
                        .width(self.size.width)
                        .height(self.size.height)
                        .layers(1),
                    None,
                ) {
                    Ok(fb) => {
                        self.framebuffers.push(fb);
                    }
                    Err(e) => {
                        for fb in self.framebuffers.iter() {
                            self.device.device.destroy_framebuffer(*fb, None);
                        }
                        return Err(e);
                    }
                }
            }
        }

        Ok(())
    }
}

impl Drop for PerSwapchain {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device.device_wait_idle();
            for fb in self.framebuffers.iter() {
                self.device.device.destroy_framebuffer(*fb, None);
            }
            for &view in self.views.iter() {
                self.device.device.destroy_image_view(view, None);
            }
            self.device
                .swapchain_loader
                .destroy_swapchain(self.swapchain, None);
        }
    }
}
