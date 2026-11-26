#version 440
// WindowView's hole: the rounded rect's coverage, as the alpha the pipeline clears the window's pixels by
// (source factor zero, destination factor one minus source alpha), so a fading item fades its hole.
layout(location = 0) in vec2 coord;
layout(location = 0) out vec4 fragColor;
layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    // top left, top right, bottom right, bottom left, in item units
    vec4 radii;
    vec2 size;
};

void main() {
    vec2 halfSize = size * 0.5;
    vec2 p = coord - halfSize;
    float k = p.x < 0.0 ? (p.y < 0.0 ? radii.x : radii.w) : (p.y < 0.0 ? radii.y : radii.z);
    k = clamp(k, 0.0, min(halfSize.x, halfSize.y));
    vec2 q = abs(p) - halfSize + vec2(k);
    float d = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - k;
    fragColor = vec4(0.0, 0.0, 0.0, clamp(0.5 - d, 0.0, 1.0) * qt_Opacity);
}
