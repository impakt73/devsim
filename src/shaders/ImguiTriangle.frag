#version 450

layout(location = 0) in vec2 vTexCoord;
layout(location = 1) in vec4 vColor;

layout(location = 0, index = 0) out vec4 OutFinalColor;

#include "Common.glsl"

void main()
{
    OutFinalColor = vColor * texture(sampler2D(uTextures[MaterialIndex()], uSampler), vTexCoord).r;
}