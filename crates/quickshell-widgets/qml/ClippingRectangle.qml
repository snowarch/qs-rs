pragma ComponentBehavior: Bound

import QtQuick
import Quickshell.Widgets

// Upstream's API over one offscreen texture instead of two: the rounded box and its border are computed in the shader
// (upstream drew a hidden Rectangle into a layer of its own and read it back), and the content's texture is kept a
// little larger than the item and grows or shrinks in steps, so a body that changes size every frame (a morph) draws
// into the texture it already has instead of getting a new one per frame.
Item {
	id: root

	property bool contentUnderBorder: false
	property bool contentInsideBorder: !root.contentUnderBorder
	property bool antialiasing: root.topLeftRadius > 0 || root.topRightRadius > 0 || root.bottomLeftRadius > 0 || root.bottomRightRadius > 0
	property color color: "white"
	property clippingRectangleBorder border
	property real radius: 0
	property real topLeftRadius: root.radius
	property real topRightRadius: root.radius
	property real bottomLeftRadius: root.radius
	property real bottomRightRadius: root.radius

	default property alias data: contentItem.data
	property alias children: contentItem.children
	readonly property alias contentItem: contentItem

	// Steps of an eighth of the size, at least 32: at most an eighth wasted, and a morph from a pill to a panel
	// changes texture about ten times instead of once a frame. Held while the item stays above half of it.
	function stepped(v: real): real {
		const step = Math.max(32, Math.pow(2, Math.floor(Math.log2(Math.max(1, v) / 8))))
		return Math.ceil(v / step) * step
	}
	property real spanWidth: root.stepped(root.width)
	property real spanHeight: root.stepped(root.height)
	onWidthChanged: if (root.width > root.spanWidth || root.width < root.spanWidth / 2) root.spanWidth = root.stepped(root.width)
	onHeightChanged: if (root.height > root.spanHeight || root.height < root.spanHeight / 2) root.spanHeight = root.stepped(root.height)

	Item {
		id: contentItemContainer
		anchors.fill: root

		Item {
			id: contentItem
			anchors.fill: parent
			anchors.margins: root.contentInsideBorder ? root.border.width : 0
		}
	}

	ShaderEffectSource {
		id: shaderSource
		hideSource: true
		sourceItem: contentItemContainer
		sourceRect: Qt.rect(0, 0, Math.max(1, root.spanWidth), Math.max(1, root.spanHeight))
	}

	ShaderEffect {
		id: shader
		anchors.fill: root
		fragmentShader: `qrc:/qt/qml/Quickshell/Widgets/shaders/cliprect${root.contentUnderBorder ? "-ub" : ""}.frag.qsb`
		property color backgroundColor: root.color
		property color borderColor: root.border.color
		property vector4d radii: Qt.vector4d(root.topLeftRadius, root.topRightRadius, root.bottomRightRadius, root.bottomLeftRadius)
		property vector2d size: Qt.vector2d(root.width, root.height)
		property vector2d region: Qt.vector2d(root.width / Math.max(1, root.spanWidth), root.height / Math.max(1, root.spanHeight))
		property real borderWidth: root.border.width
		property real smoothing: root.antialiasing ? 1 : 0
		property ShaderEffectSource content: shaderSource
	}
}
