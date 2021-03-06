use crate::device::*;
use crate::swapsurface::*;
use crate::perframe::*;

use winit::window::Window;
use std::rc::Rc;



pub struct VulkanWindow {
    pub window: Window,
    pub surface: Rc<Surface>,
    pub device: Rc<Device>,
    pub swap: PerSwapchain,
    pub per_frame: Vec<PerFrame>,

    pub frame_count: usize,
    pub count_start_time: std::time::Instant,
    pub count_start_frame: usize,

    pub anim_start_time: std::time::Instant,
    pub shape_rotate_speed: f32,
    pub color_rotate_speed: f32,
    pub background_color: [f32; 4]
}

impl Drop for VulkanWindow {
    fn drop(&mut self) {
    }
}

