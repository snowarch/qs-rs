#version 440
// WindowView's hole: a rounded rect, with its coordinates in item units for the fragment stage.
layout(location = 0) in vec4 qt_Vertex;
layout(location = 1) in vec2 qt_MultiTexCoord0;
layout(location = 0) out vec2 coord;
layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    // top left, top right, bottom right, bottom left, in item units
    vec4 radii;
    vec2 size;
};

void main() {
    coord = qt_MultiTexCoord0 * size;
    gl_Position = qt_Matrix * qt_Vertex;
}
