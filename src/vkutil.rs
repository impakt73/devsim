use ash::{
    extensions::{ext, khr},
    version::{DeviceV1_0, EntryV1_0, InstanceV1_0},
    vk,
};
use std::borrow::Cow;
use std::ffi::{CStr, CString};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _user_data: *mut std::os::raw::c_void,
) -> vk::Bool32 {
    let callback_data = *p_callback_data;
    let message_id_number: i32 = callback_data.message_id_number as i32;

    let message_id_name = if callback_data.p_message_id_name.is_null() {
        Cow::from("")
    } else {
        CStr::from_ptr(callback_data.p_message_id_name).to_string_lossy()
    };

    let message = if callback_data.p_message.is_null() {
        Cow::from("")
    } else {
        CStr::from_ptr(callback_data.p_message).to_string_lossy()
    };

    println!(
        "{:?}:\n{:?} [{} ({})] : {}\n",
        message_severity,
        message_type,
        message_id_name,
        &message_id_number.to_string(),
        message,
    );

    vk::FALSE
}

pub struct ComputePipeline {
    inner: vk::Pipeline,
}

impl ComputePipeline {
    pub fn from_glsl(
        device: ash::Device,
        pipeline_layout: vk::PipelineLayout,
        source_text: &str,
        source_filename: &str,
        entry_point: &str,
    ) -> Result<Self> {
        let mut compiler = shaderc::Compiler::new().ok_or("Failed to initialize spirv compiler")?;

        let artifact = compiler.compile_into_spirv(
            source_text,
            shaderc::ShaderKind::Compute,
            source_filename,
            entry_point,
            None,
        )?;

        Self::from_spv(device, pipeline_layout, artifact.as_binary(), entry_point)
    }

    pub fn from_spv(
        device: ash::Device,
        pipeline_layout: vk::PipelineLayout,
        spv_binary: &[u32],
        entry_point: &str,
    ) -> Result<Self> {
        let entry_point_c_string = CString::new(entry_point)?;

        unsafe {
            let module = device.create_shader_module(
                &vk::ShaderModuleCreateInfo::builder().code(spv_binary),
                None,
            )?;

            let compile_result = device.create_compute_pipelines(
                vk::PipelineCache::null(),
                &[vk::ComputePipelineCreateInfo::builder()
                    .stage(
                        vk::PipelineShaderStageCreateInfo::builder()
                            .stage(vk::ShaderStageFlags::COMPUTE)
                            .module(module)
                            .name(entry_point_c_string.as_c_str())
                            .build(),
                    )
                    .layout(pipeline_layout)
                    .build()],
                None,
            );
            device.destroy_shader_module(module, None);

            match compile_result {
                Ok(compute_pipelines) => {
                    let compute_pipeline = ComputePipeline {
                        inner: compute_pipelines[0],
                    };

                    Ok(compute_pipeline)
                }
                Err(err) => Err(err.1.into()),
            }
        }
    }

    pub fn raw(&self) -> vk::Pipeline {
        self.inner
    }
}

/// Returns a vector of all desired instance extensions
fn select_instance_extensions(surface_extensions: Vec<CString>) -> Vec<CString> {
    let mut exts = Vec::new();

    // Add in all required surface extensions
    exts.extend(surface_extensions);

    // Add the debug utils extension
    let debug_utils_ext_name = CString::new(ext::DebugUtils::name().to_bytes()).unwrap();
    exts.push(debug_utils_ext_name);

    exts
}

/// Returns a vector of all desired instance layers
fn select_instance_layers(enable_validation: bool) -> Vec<CString> {
    let mut exts = Vec::new();

    // If the caller wants API validation, make sure we add the instance layer here
    if enable_validation {
        exts.push(CString::new("VK_LAYER_KHRONOS_validation").unwrap());
    }

    exts
}

/// Returns a vector of all desired device extensions
fn select_device_extensions() -> Vec<CString> {
    let mut exts = Vec::new();

    // Add the swapchain extension
    let swapchain_ext_name = CString::new(khr::Swapchain::name().to_bytes()).unwrap();
    exts.push(swapchain_ext_name);

    exts
}

struct VkSurface {
    inner: vk::SurfaceKHR,
    ext: khr::Surface,
}

impl VkSurface {
    fn new(instance: &VkInstance, window: &winit::window::Window) -> Result<Self> {
        unsafe {
            // Create a surface from winit window.
            let surface =
                ash_window::create_surface(&instance.entry, &instance.inner, window, None)?;
            let ext = khr::Surface::new(&instance.entry, &instance.inner);

            Ok(Self {
                inner: surface,
                ext,
            })
        }
    }
}

impl Drop for VkSurface {
    fn drop(&mut self) {
        unsafe {
            self.ext.destroy_surface(self.inner, None);
        }
    }
}

/// Selects a physical device from the provided list
fn select_physical_device(physical_devices: &[vk::PhysicalDevice]) -> vk::PhysicalDevice {
    // TODO: Support proper physical device selection
    //       For now, we just use the first device
    physical_devices[0]
}

// Wrapper structure used to create and manage a Vulkan instance
struct VkInstance {
    inner: ash::Instance,
    entry: ash::Entry,
}

impl VkInstance {
    fn new(window: &winit::window::Window, enable_validation: bool) -> Result<Self> {
        unsafe {
            let entry = ash::Entry::new()?;
            // Query all extensions required for swapchain usage
            let surface_extensions = ash_window::enumerate_required_extensions(window)?
                .iter()
                .map(|ext| CString::new(ext.to_bytes()).unwrap())
                .collect::<Vec<_>>();
            let instance_extension_strings = select_instance_extensions(surface_extensions);
            let instance_extensions = instance_extension_strings
                .iter()
                .map(|ext| ext.as_ptr())
                .collect::<Vec<_>>();
            let instance_layer_strings = select_instance_layers(enable_validation);
            let instance_layers = instance_layer_strings
                .iter()
                .map(|ext| ext.as_ptr())
                .collect::<Vec<_>>();
            let app_desc = vk::ApplicationInfo::builder().api_version(vk::make_version(1, 2, 0));
            let instance_desc = vk::InstanceCreateInfo::builder()
                .application_info(&app_desc)
                .enabled_extension_names(&instance_extensions)
                .enabled_layer_names(&instance_layers);

            let instance = entry.create_instance(&instance_desc, None)?;

            Ok(Self {
                inner: instance,
                entry,
            })
        }
    }
}

impl Drop for VkInstance {
    fn drop(&mut self) {
        unsafe {
            self.inner.destroy_instance(None);
        }
    }
}

struct VkDebugMessenger {
    inner: vk::DebugUtilsMessengerEXT,
    debug_messenger_ext: ext::DebugUtils,
}

impl VkDebugMessenger {
    fn new(instance: &VkInstance) -> Result<Self> {
        unsafe {
            let debug_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
                .message_severity(
                    vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
                )
                .message_type(vk::DebugUtilsMessageTypeFlagsEXT::all())
                .pfn_user_callback(Some(vulkan_debug_callback));

            let debug_messenger_ext = ext::DebugUtils::new(&instance.entry, &instance.inner);
            let debug_messenger =
                debug_messenger_ext.create_debug_utils_messenger(&debug_info, None)?;

            Ok(Self {
                inner: debug_messenger,
                debug_messenger_ext,
            })
        }
    }
}

impl Drop for VkDebugMessenger {
    fn drop(&mut self) {
        unsafe {
            self.debug_messenger_ext
                .destroy_debug_utils_messenger(self.inner, None);
        }
    }
}

/// Returns the index for the best queue family to use given the provided usage flags
fn find_best_queue_for_usage(
    queue_properties: &[vk::QueueFamilyProperties],
    usage: vk::QueueFlags,
) -> usize {
    let mut queue_index = usize::MAX;
    let mut min_support_bits = u32::MAX;
    for (idx, queue_properties) in queue_properties.iter().enumerate() {
        if queue_properties.queue_flags.contains(usage) {
            let support_bits = queue_properties.queue_flags.as_raw().count_ones();
            if support_bits < min_support_bits {
                min_support_bits = support_bits;
                queue_index = idx;
            }
        }
    }

    queue_index as usize
}

enum VkQueueType {
    Graphics = 0,
    Compute,
    Transfer,
}
const VK_QUEUE_TYPE_COUNT: usize = 3;

struct VkDevice {
    inner: ash::Device,
    physical_device: vk::PhysicalDevice,
    queues: Vec<vk::Queue>,
    queues_by_type: [vk::Queue; VK_QUEUE_TYPE_COUNT],
    present_queue: vk::Queue,
}

impl VkDevice {
    fn new(
        instance: &VkInstance,
        physical_device: vk::PhysicalDevice,
        surface: &VkSurface,
    ) -> Result<Self> {
        unsafe {
            let device_extension_strings = select_device_extensions();
            let device_extensions = device_extension_strings
                .iter()
                .map(|ext| ext.as_ptr())
                .collect::<Vec<_>>();

            let queue_family_properties = instance
                .inner
                .get_physical_device_queue_family_properties(physical_device);

            // Identify a suitable queue family index for presentation
            let mut present_queue_family_index = u32::MAX;
            for idx in 0..queue_family_properties.len() {
                if surface.ext.get_physical_device_surface_support(
                    physical_device,
                    idx as u32,
                    surface.inner,
                )? {
                    present_queue_family_index = idx as u32;
                    break;
                }
            }

            // Initialize all available queue types
            let queue_infos = queue_family_properties
                .iter()
                .enumerate()
                .map(|(idx, _info)| {
                    vk::DeviceQueueCreateInfo::builder()
                        .queue_family_index(idx as u32)
                        .queue_priorities(&[1.0])
                        .build()
                })
                .collect::<Vec<_>>();

            let device_create_info = vk::DeviceCreateInfo::builder()
                .queue_create_infos(&queue_infos)
                .enabled_extension_names(&device_extensions);

            let device =
                instance
                    .inner
                    .create_device(physical_device, &device_create_info, None)?;

            let queues = queue_infos
                .iter()
                .enumerate()
                .map(|(idx, _info)| device.get_device_queue(idx as u32, 0))
                .collect::<Vec<_>>();

            let queues_by_type = [
                queues
                    [find_best_queue_for_usage(&queue_family_properties, vk::QueueFlags::GRAPHICS)],
                queues
                    [find_best_queue_for_usage(&queue_family_properties, vk::QueueFlags::COMPUTE)],
                queues
                    [find_best_queue_for_usage(&queue_family_properties, vk::QueueFlags::TRANSFER)],
            ];

            let present_queue = queues[present_queue_family_index as usize];

            Ok(Self {
                inner: device,
                physical_device,
                queues,
                queues_by_type,
                present_queue,
            })
        }
    }

    fn graphics_queue(&self) -> vk::Queue {
        self.queues_by_type[VkQueueType::Graphics as usize]
    }

    fn compute_queue(&self) -> vk::Queue {
        self.queues_by_type[VkQueueType::Compute as usize]
    }

    fn transfer_queue(&self) -> vk::Queue {
        self.queues_by_type[VkQueueType::Transfer as usize]
    }

    fn present_queue(&self) -> vk::Queue {
        self.present_queue
    }
}

impl Drop for VkDevice {
    fn drop(&mut self) {
        unsafe {
            self.inner.destroy_device(None);
        }
    }
}

pub struct VkSwapchain {
    inner: vk::SwapchainKHR,
    ext: ash::extensions::khr::Swapchain,
    pub surface_format: vk::SurfaceFormatKHR,
    pub surface_resolution: vk::Extent2D,
}

impl VkSwapchain {
    fn new(
        instance: &VkInstance,
        device: &VkDevice,
        surface: &VkSurface,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        unsafe {
            let surface_formats = surface
                .ext
                .get_physical_device_surface_formats(device.physical_device, surface.inner)?;
            let surface_format = if (surface_formats.len() == 1)
                && (surface_formats[0].format == vk::Format::UNDEFINED)
            {
                // Undefined means we get to choose our format
                vk::SurfaceFormatKHR::builder()
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
                    .build()
            } else {
                // Attempt to select R8G8B8A8
                if let Some(format) = surface_formats
                    .iter()
                    .find(|surface| surface.format == vk::Format::R8G8B8A8_UNORM)
                {
                    *format
                // Fall back to B8R8G8A8
                } else if let Some(format) = surface_formats
                    .iter()
                    .find(|surface| surface.format == vk::Format::B8G8R8A8_UNORM)
                {
                    *format
                // If everything else fails, just use the first format in the list
                } else {
                    surface_formats[0]
                }
            };
            let surface_capabilities = surface
                .ext
                .get_physical_device_surface_capabilities(device.physical_device, surface.inner)?;
            let mut desired_image_count = surface_capabilities.min_image_count + 1;
            if surface_capabilities.max_image_count > 0
                && desired_image_count > surface_capabilities.max_image_count
            {
                desired_image_count = surface_capabilities.max_image_count;
            }
            let surface_resolution = match surface_capabilities.current_extent.width {
                std::u32::MAX => vk::Extent2D { width, height },
                _ => surface_capabilities.current_extent,
            };
            let pre_transform = if surface_capabilities
                .supported_transforms
                .contains(vk::SurfaceTransformFlagsKHR::IDENTITY)
            {
                vk::SurfaceTransformFlagsKHR::IDENTITY
            } else {
                surface_capabilities.current_transform
            };
            let present_modes = surface
                .ext
                .get_physical_device_surface_present_modes(device.physical_device, surface.inner)?;
            let present_mode = if present_modes.contains(&vk::PresentModeKHR::MAILBOX) {
                // Prefer mailbox mode
                vk::PresentModeKHR::MAILBOX
            } else if present_modes.contains(&vk::PresentModeKHR::FIFO_RELAXED) {
                // Use fifo relaxed if mailbox isn't available
                vk::PresentModeKHR::FIFO_RELAXED
            } else {
                // Fall back to the required fifo mode if nothing else works
                vk::PresentModeKHR::FIFO
            };
            let ext = khr::Swapchain::new(&instance.inner, &device.inner);

            let swapchain_create_info = vk::SwapchainCreateInfoKHR::builder()
                .surface(surface.inner)
                .min_image_count(desired_image_count)
                .image_color_space(surface_format.color_space)
                .image_format(surface_format.format)
                .image_extent(surface_resolution)
                .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
                .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                .pre_transform(pre_transform)
                .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                .present_mode(present_mode)
                .clipped(true)
                .image_array_layers(1);

            let swapchain = ext.create_swapchain(&swapchain_create_info, None)?;

            Ok(Self {
                inner: swapchain,
                ext,
                surface_format,
                surface_resolution,
            })
        }
    }

    /// Attempts to acquire the next image in the swapchain
    pub fn acquire_next_image(
        &self,
        timeout: u64,
        semaphore: Option<vk::Semaphore>,
        fence: Option<vk::Fence>,
    ) -> Result<(u32, bool)> {
        unsafe {
            Ok(self.ext.acquire_next_image(
                self.inner,
                timeout,
                semaphore.unwrap_or_default(),
                fence.unwrap_or_default(),
            )?)
        }
    }

    // Attempts to present the specified swapchain image on the display
    pub fn present_image(
        &self,
        index: u32,
        wait_semaphores: &[vk::Semaphore],
        queue: vk::Queue,
    ) -> Result<bool> {
        let swapchains = [self.inner];
        let image_indices = [index];
        let present_info = vk::PresentInfoKHR::builder()
            .wait_semaphores(wait_semaphores)
            .swapchains(&swapchains)
            .image_indices(&image_indices);

        unsafe { Ok(self.ext.queue_present(queue, &present_info)?) }
    }
}

impl Drop for VkSwapchain {
    fn drop(&mut self) {
        unsafe {
            self.ext.destroy_swapchain(self.inner, None);
        }
    }
}

pub struct VkBuffer<'a> {
    inner: vk::Buffer,
    allocator: &'a vk_mem::Allocator,
    allocation: vk_mem::Allocation,
}

impl<'a> VkBuffer<'a> {
    pub fn new(
        allocator: &'a vk_mem::Allocator,
        buffer_info: &vk::BufferCreateInfo,
        allocation_info: &vk_mem::AllocationCreateInfo,
    ) -> Result<Self> {
        let (inner, allocation, _alloc_info) =
            allocator.create_buffer(buffer_info, allocation_info)?;
        Ok(VkBuffer {
            inner,
            allocator,
            allocation,
        })
    }

    pub fn raw(&self) -> vk::Buffer {
        self.inner
    }
}

impl<'a> Drop for VkBuffer<'a> {
    fn drop(&mut self) {
        // TODO: This function always returns a successful result and should be modified to not
        //       return anything.
        self.allocator
            .destroy_buffer(self.inner, &self.allocation)
            .unwrap();
    }
}

pub struct VkImage<'a> {
    inner: vk::Image,
    allocator: &'a vk_mem::Allocator,
    allocation: vk_mem::Allocation,
}

impl<'a> VkImage<'a> {
    pub fn new(
        allocator: &'a vk_mem::Allocator,
        image_info: &vk::ImageCreateInfo,
        allocation_info: &vk_mem::AllocationCreateInfo,
    ) -> Result<Self> {
        let (inner, allocation, _alloc_info) =
            allocator.create_image(image_info, allocation_info)?;
        Ok(VkImage {
            inner,
            allocator,
            allocation,
        })
    }

    pub fn raw(&self) -> vk::Image {
        self.inner
    }
}

impl<'a> Drop for VkImage<'a> {
    fn drop(&mut self) {
        // TODO: This function always returns a successful result and should be modified to not
        //       return anything.
        self.allocator
            .destroy_image(self.inner, &self.allocation)
            .unwrap();
    }
}

pub struct VkImageView<'a> {
    inner: vk::ImageView,
    device: &'a ash::Device,
}

impl<'a> VkImageView<'a> {
    pub fn new(device: &'a ash::Device, create_info: &vk::ImageViewCreateInfo) -> Result<Self> {
        let inner = unsafe { device.create_image_view(create_info, None)? };
        Ok(VkImageView { inner, device })
    }

    pub fn raw(&self) -> vk::ImageView {
        self.inner
    }
}

impl<'a> Drop for VkImageView<'a> {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_image_view(self.inner, None);
        }
    }
}

pub struct VkSampler<'a> {
    inner: vk::Sampler,
    device: &'a ash::Device,
}

impl<'a> VkSampler<'a> {
    pub fn new(device: &'a ash::Device, create_info: &vk::SamplerCreateInfo) -> Result<Self> {
        let inner = unsafe { device.create_sampler(create_info, None)? };
        Ok(VkSampler { inner, device })
    }

    pub fn raw(&self) -> vk::Sampler {
        self.inner
    }
}

impl<'a> Drop for VkSampler<'a> {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_sampler(self.inner, None);
        }
    }
}

pub struct VkCommandPool<'a> {
    inner: vk::CommandPool,
    device: &'a ash::Device,
}

impl<'a> VkCommandPool<'a> {
    pub fn new(device: &'a ash::Device, create_info: &vk::CommandPoolCreateInfo) -> Result<Self> {
        let inner = unsafe { device.create_command_pool(create_info, None)? };
        Ok(VkCommandPool { inner, device })
    }

    pub fn raw(&self) -> vk::CommandPool {
        self.inner
    }
}

impl<'a> Drop for VkCommandPool<'a> {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_command_pool(self.inner, None);
        }
    }
}

pub struct VkSemaphore<'a> {
    inner: vk::Semaphore,
    device: &'a ash::Device,
}

impl<'a> VkSemaphore<'a> {
    pub fn new(device: &'a ash::Device, create_info: &vk::SemaphoreCreateInfo) -> Result<Self> {
        let inner = unsafe { device.create_semaphore(create_info, None)? };
        Ok(VkSemaphore { inner, device })
    }

    pub fn raw(&self) -> vk::Semaphore {
        self.inner
    }
}

impl<'a> Drop for VkSemaphore<'a> {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_semaphore(self.inner, None);
        }
    }
}

pub struct VkFence<'a> {
    inner: vk::Fence,
    device: &'a ash::Device,
}

impl<'a> VkFence<'a> {
    pub fn new(device: &'a ash::Device, create_info: &vk::FenceCreateInfo) -> Result<Self> {
        let inner = unsafe { device.create_fence(create_info, None)? };
        Ok(VkFence { inner, device })
    }

    pub fn raw(&self) -> vk::Fence {
        self.inner
    }
}

impl<'a> Drop for VkFence<'a> {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_fence(self.inner, None);
        }
    }
}

pub struct VkDescriptorSetLayout<'a> {
    inner: vk::DescriptorSetLayout,
    device: &'a ash::Device,
}

impl<'a> VkDescriptorSetLayout<'a> {
    pub fn new(
        device: &'a ash::Device,
        create_info: &vk::DescriptorSetLayoutCreateInfo,
    ) -> Result<Self> {
        let inner = unsafe { device.create_descriptor_set_layout(create_info, None)? };
        Ok(VkDescriptorSetLayout { inner, device })
    }

    pub fn raw(&self) -> vk::DescriptorSetLayout {
        self.inner
    }
}

impl<'a> Drop for VkDescriptorSetLayout<'a> {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_descriptor_set_layout(self.inner, None);
        }
    }
}

pub struct VkPipelineLayout<'a> {
    inner: vk::PipelineLayout,
    device: &'a ash::Device,
}

impl<'a> VkPipelineLayout<'a> {
    pub fn new(
        device: &'a ash::Device,
        create_info: &vk::PipelineLayoutCreateInfo,
    ) -> Result<Self> {
        let inner = unsafe { device.create_pipeline_layout(create_info, None)? };
        Ok(VkPipelineLayout { inner, device })
    }

    pub fn raw(&self) -> vk::PipelineLayout {
        self.inner
    }
}

impl<'a> Drop for VkPipelineLayout<'a> {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_pipeline_layout(self.inner, None);
        }
    }
}

pub struct VkDescriptorPool<'a> {
    inner: vk::DescriptorPool,
    device: &'a ash::Device,
}

impl<'a> VkDescriptorPool<'a> {
    pub fn new(
        device: &'a ash::Device,
        create_info: &vk::DescriptorPoolCreateInfo,
    ) -> Result<Self> {
        let inner = unsafe { device.create_descriptor_pool(create_info, None)? };
        Ok(VkDescriptorPool { inner, device })
    }

    pub fn raw(&self) -> vk::DescriptorPool {
        self.inner
    }
}

impl<'a> Drop for VkDescriptorPool<'a> {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_descriptor_pool(self.inner, None);
        }
    }
}

pub struct VkShaderModule<'a> {
    inner: vk::ShaderModule,
    device: &'a ash::Device,
}

impl<'a> VkShaderModule<'a> {
    pub fn new(device: &'a ash::Device, create_info: &vk::ShaderModuleCreateInfo) -> Result<Self> {
        let inner = unsafe { device.create_shader_module(create_info, None)? };
        Ok(VkShaderModule { inner, device })
    }

    pub fn raw(&self) -> vk::ShaderModule {
        self.inner
    }
}

impl<'a> Drop for VkShaderModule<'a> {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_shader_module(self.inner, None);
        }
    }
}

pub struct VkPipelineCache<'a> {
    inner: vk::PipelineCache,
    device: &'a ash::Device,
}

impl<'a> VkPipelineCache<'a> {
    pub fn new(device: &'a ash::Device, create_info: &vk::PipelineCacheCreateInfo) -> Result<Self> {
        let inner = unsafe { device.create_pipeline_cache(create_info, None)? };
        Ok(VkPipelineCache { inner, device })
    }

    pub fn raw(&self) -> vk::PipelineCache {
        self.inner
    }
}

impl<'a> Drop for VkPipelineCache<'a> {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_pipeline_cache(self.inner, None);
        }
    }
}

pub struct VkRenderPass<'a> {
    inner: vk::RenderPass,
    device: &'a ash::Device,
}

impl<'a> VkRenderPass<'a> {
    pub fn new(device: &'a ash::Device, create_info: &vk::RenderPassCreateInfo) -> Result<Self> {
        let inner = unsafe { device.create_render_pass(create_info, None)? };
        Ok(VkRenderPass { inner, device })
    }

    pub fn raw(&self) -> vk::RenderPass {
        self.inner
    }
}

impl<'a> Drop for VkRenderPass<'a> {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_render_pass(self.inner, None);
        }
    }
}

pub struct VkFramebuffer<'a> {
    inner: vk::Framebuffer,
    device: &'a ash::Device,
}

impl<'a> VkFramebuffer<'a> {
    pub fn new(device: &'a ash::Device, create_info: &vk::FramebufferCreateInfo) -> Result<Self> {
        let inner = unsafe { device.create_framebuffer(create_info, None)? };
        Ok(VkFramebuffer { inner, device })
    }

    pub fn raw(&self) -> vk::Framebuffer {
        self.inner
    }
}

impl<'a> Drop for VkFramebuffer<'a> {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_framebuffer(self.inner, None);
        }
    }
}

// Wrapper structure used to load and manage a logical Vulkan device
pub struct RenderDevice {
    pub allocator: vk_mem::Allocator,
    pub swapchain_image_views: Vec<vk::ImageView>,
    pub swapchain_images: Vec<vk::Image>,
    pub swapchain: VkSwapchain,
    surface: VkSurface,
    device: VkDevice,
    debug_messenger: Option<VkDebugMessenger>,
    instance: VkInstance,
}

impl RenderDevice {
    pub fn new(window: &winit::window::Window, enable_validation: bool) -> Result<Self> {
        unsafe {
            let instance = VkInstance::new(window, enable_validation)?;

            let debug_messenger = if enable_validation {
                Some(VkDebugMessenger::new(&instance)?)
            } else {
                None
            };

            let physical_devices = instance.inner.enumerate_physical_devices()?;
            let physical_device = select_physical_device(&physical_devices);

            let surface = VkSurface::new(&instance, window)?;

            let device = VkDevice::new(&instance, physical_device, &surface)?;

            let swapchain = VkSwapchain::new(
                &instance,
                &device,
                &surface,
                window.inner_size().width,
                window.inner_size().height,
            )?;

            let swapchain_images = swapchain.ext.get_swapchain_images(swapchain.inner)?;
            let mut swapchain_image_views = Vec::new();
            for swapchain_image in &swapchain_images {
                let image_view = device.inner.create_image_view(
                    &vk::ImageViewCreateInfo::builder()
                        .image(*swapchain_image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(swapchain.surface_format.format)
                        .components(
                            vk::ComponentMapping::builder()
                                .r(vk::ComponentSwizzle::IDENTITY)
                                .g(vk::ComponentSwizzle::IDENTITY)
                                .b(vk::ComponentSwizzle::IDENTITY)
                                .a(vk::ComponentSwizzle::IDENTITY)
                                .build(),
                        )
                        .subresource_range(
                            vk::ImageSubresourceRange::builder()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .base_mip_level(0)
                                .level_count(1)
                                .base_array_layer(0)
                                .layer_count(1)
                                .build(),
                        ),
                    None,
                )?;
                swapchain_image_views.push(image_view);
            }

            let allocator = vk_mem::Allocator::new(&vk_mem::AllocatorCreateInfo {
                physical_device,
                device: device.inner.clone(),
                instance: instance.inner.clone(),
                flags: vk_mem::AllocatorCreateFlags::NONE,
                preferred_large_heap_block_size: 0,
                frame_in_use_count: 0,
                heap_size_limits: None,
            })?;

            Ok(Self {
                allocator,
                swapchain_image_views,
                swapchain_images,
                swapchain,
                surface,
                device,
                debug_messenger,
                instance,
            })
        }
    }

    /// Returns the internal ash device
    pub fn raw(&self) -> &ash::Device {
        &self.device.inner
    }

    /// Returns the present queue for the device
    pub fn get_present_queue(&self) -> vk::Queue {
        self.device.present_queue()
    }

    /// Returns the queue family index for the graphics queue
    pub fn get_graphics_family_index(&self) -> u32 {
        // TODO: Return the correct family index
        0
    }
}
