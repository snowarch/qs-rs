use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    let singletons = [
        "qml/Networking.qml",
        "qml/ConnectionFailReason.qml",
        "qml/ConnectionState.qml",
        "qml/DeviceType.qml",
        "qml/NetworkBackendType.qml",
        "qml/NetworkConnectivity.qml",
        "qml/WifiDeviceMode.qml",
        "qml/WifiSecurityType.qml",
    ];
    let mut module = QmlModule::new("Quickshell.Networking").version(0, 1).qml_files([
        "qml/NetworkDevice.qml",
        "qml/WifiDevice.qml",
        "qml/WiredDevice.qml",
        "qml/Network.qml",
        "qml/WifiNetwork.qml",
        "qml/NMSettings.qml",
    ]);
    for s in singletons {
        module = module.qml_file(QmlFile::from(s).singleton(true));
    }
    CxxQtBuilder::new_qml_module(module).files(["src/core.rs"]).build().export();
}
