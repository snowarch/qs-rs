pragma ComponentBehavior: Bound

import QtQuick
import Quickshell.Widgets

Item {
	id: root

	property bool contentUnderBorder: false
	property bool contentInsideBorder: !root.contentUnderBorder
	property /*bool*/alias antialiasing: rectangle.antialiasing
	property /*color*/alias color: shader.backgroundColor
	property clippingRectangleBorder border
	property /*real*/alias radius: rectangle.radius
	property /*real*/alias topLeftRadius: rectangle.topLeftRadius
	property /*real*/alias topRightRadius: rectangle.topRightRadius
	property /*real*/alias bottomLeftRadius: rectangle.bottomLeftRadius
	property /*real*/alias bottomRightRadius: rectangle.bottomRightRadius

	default property alias data: contentItem.data
	property alias children: contentItem.children
	readonly property alias contentItem: contentItem

	Rectangle {
		id: rectangle
		anchors.fill: root
		color: "#ffff0000"
		border.color: "#ff00ff00"
		border.pixelAligned: root.border.pixelAligned
		border.width: root.border.width
		layer.enabled: true
		visible: false
	}

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
	}

	ShaderEffect {
		id: shader
		anchors.fill: root
		fragmentShader: `qrc:/qt/qml/Quickshell/Widgets/shaders/cliprect${root.contentUnderBorder ? "-ub" : ""}.frag.qsb`
		property Rectangle rect: rectangle
		property color backgroundColor: "white"
		property color borderColor: root.border.color
		property ShaderEffectSource content: shaderSource
	}
}
