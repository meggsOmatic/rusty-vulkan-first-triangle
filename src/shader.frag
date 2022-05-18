#version 450

layout(location = 0) in vec2 colorCoords;

layout(location = 0) out vec4 outColor;

layout( push_constant ) uniform constants
{
    float width_scale;
    float height_scale;
    float shape_rotate;
    float color_rotate;
} PushConstants;

void main() {
    float theta = atan(colorCoords.y, colorCoords.x) + PushConstants.color_rotate;
    float r = smoothstep(0, 0.25, length(colorCoords));
    vec3 c = vec3(cos(theta), cos(theta + radians(120)), cos(theta + radians(240)));
    c = vec3(0.5) + 0.5 * c;
    c = mix(vec3(0.5), c, r);
    outColor = vec4(c, 1.0);
}
