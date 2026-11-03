#version 440
// ClippingRectangle: the content's texture shown through a rounded box with a border, the box computed here.
// Upstream read the shape back from a Rectangle drawn into a second offscreen texture, and both textures were
// made again on every resize: every frame of a body that grows or shrinks.
layout(location = 0) in vec2 qt_TexCoord0;
layout(location = 0) out vec4 fragColor;

layout(std140, binding = 0) uniform buf {
	mat4 qt_Matrix;
	float qt_Opacity;
	vec4 backgroundColor;
	vec4 borderColor;
	// top left, top right, bottom right, bottom left, in item units
	vec4 radii;
	// the item, in item units
	vec2 size;
	// the share of the content texture the item covers (it is kept larger while the item resizes)
	vec2 region;
	float borderWidth;
	float smoothing;
};

layout(binding = 1) uniform sampler2D content;

float roundedBox(vec2 p, vec2 halfSize, vec4 r) {
	float k = p.x < 0.0 ? (p.y < 0.0 ? r.x : r.w) : (p.y < 0.0 ? r.y : r.z);
	k = clamp(k, 0.0, min(halfSize.x, halfSize.y));
	vec2 q = abs(p) - halfSize + k;
	return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - k;
}

void main() {
	vec2 p = (qt_TexCoord0 - 0.5) * size;
	float d = roundedBox(p, size * 0.5, radii);
	// One device pixel of ramp where antialiased, a hard edge at the pixel centre otherwise.
	float ramp = smoothing > 0.5 ? max(fwidth(d), 1e-4) : 1e-4;
	float shape = clamp(0.5 - d / ramp, 0.0, 1.0);
	float inner = borderWidth > 0.0 ? clamp(0.5 - (d + borderWidth) / ramp, 0.0, 1.0) : shape;

	vec4 contentColor = texture(content, qt_TexCoord0 * region);
#ifdef CONTENT_UNDER_BORDER
	float contentAlpha = shape;
#else
	float contentAlpha = inner;
#endif
	vec4 innerColor = (contentColor + backgroundColor * (1.0 - contentColor.a)) * contentAlpha;
	vec4 edge = borderColor * (shape - inner);
	fragColor = (innerColor * (1.0 - edge.a) + edge) * qt_Opacity;
}
