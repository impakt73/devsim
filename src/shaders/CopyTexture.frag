#version 450

layout(location = 0) in vec2 vTexCoord;

layout(location = 0, index = 0) out vec4 OutFinalColor;

layout(set = 0, binding = 0) uniform sampler uSampler;
layout(set = 0, binding = 2) uniform texture2D uTexture;

void main()
{
    OutFinalColor = vec4(texture(sampler2D(uTexture, uSampler), vTexCoord).rgb, 1.0);
}