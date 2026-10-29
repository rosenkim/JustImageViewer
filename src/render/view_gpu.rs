/// GPU services borrowed only while a content panel prepares its frame.
pub struct ViewGpuContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub renderer: &'a mut imgui_wgpu::Renderer,
}
