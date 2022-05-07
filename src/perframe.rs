use crate::device::*;


use ash::prelude::*;
use ash::vk;
use std::default::Default;
use std::rc::Rc;


pub struct PerFrame {
    pub device: Rc<Device>,
    pub command_buffer: vk::CommandBuffer,
    pub image_available_semaphore: vk::Semaphore,
    pub render_finished_semaphore: vk::Semaphore,
    pub in_flight_fence: vk::Fence,
}

impl PerFrame {
    pub fn new(device: Rc<Device>) -> VkResult<PerFrame> {
        unsafe {
            let command_buffer = device.device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(device.command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            );

            let image_available_semaphore = match command_buffer {
                Ok(_) => device
                    .device
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None),
                Err(e) => Err(e),
            };

            let render_finished_semaphore = image_available_semaphore.and_then(|_| {
                device
                    .device
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
            });

            let in_flight_fence = render_finished_semaphore.and_then(|_| {
                device.device.create_fence(
                    &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )
            });

            if in_flight_fence.is_ok() {
                return Ok(PerFrame {
                    device,
                    command_buffer: command_buffer.unwrap()[0],
                    image_available_semaphore: image_available_semaphore.unwrap(),
                    render_finished_semaphore: render_finished_semaphore.unwrap(),
                    in_flight_fence: in_flight_fence.unwrap(),
                });
            }

            if let Ok(f) = in_flight_fence {
                device.device.destroy_fence(f, None);
            }

            if let Ok(s) = render_finished_semaphore {
                device.device.destroy_semaphore(s, None);
            }

            if let Ok(s) = image_available_semaphore {
                device.device.destroy_semaphore(s, None);
            }

            if let Ok(c) = command_buffer {
                device.device.free_command_buffers(device.command_pool, &c);
            }

            Err(in_flight_fence.unwrap_err())
        }
    }
}

impl Drop for PerFrame {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .device
                .device
                .wait_for_fences(&[self.in_flight_fence], true, 100_000_000);
            self.device
                .device
                .destroy_semaphore(self.image_available_semaphore, None);
            self.device
                .device
                .destroy_semaphore(self.render_finished_semaphore, None);
            self.device
                .device
                .free_command_buffers(self.device.command_pool, &[self.command_buffer]);
            self.device.device.destroy_fence(self.in_flight_fence, None);
        }
    }
}

