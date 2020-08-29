use ash::{version::DeviceV1_0, vk};
use std::ffi::CString;
use serde::{Serialize, Deserialize};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Serialize, Deserialize, Debug)]
pub struct SamplerInfo {}

#[derive(Serialize, Deserialize, Debug)]
pub struct DescriptorSetBinding {
    _type: String,
    count: u32,
    stages: Vec<String>,
    immutable_samplers: Vec<SamplerInfo>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PushConstantRange {
    stages: Vec<String>,
    offset: u32,
    size: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PipelineLayout {
    descriptor_sets: Vec<DescriptorSetBinding>,
    push_constant_ranges: Vec<PushConstantRange>,
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
