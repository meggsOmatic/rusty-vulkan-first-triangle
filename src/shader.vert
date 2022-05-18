#version 450

layout(location = 0) in vec2 inPosition;
layout(location = 1) in vec3 inColor;

layout(location = 0) out vec2 colorCoords;

layout( push_constant ) uniform constants
{
    float width_scale;
    float height_scale;
    float shape_rotate;
    float color_rotate;
} PushConstants;


void main() {
    float theta = gl_VertexIndex * (3.14159265 * 2.0 / 3.0) + PushConstants.shape_rotate;
    colorCoords = vec2(cos(theta), sin(theta));
    gl_Position = vec4(PushConstants.width_scale * colorCoords.x, PushConstants.height_scale * colorCoords.y, 0.0, 1.0);
}