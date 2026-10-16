use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell.Bluetooth")
            .version(0, 1)
            .qml_files(["qml/BluetoothAdapter.qml", "qml/BluetoothDevice.qml"])
            .qml_file(QmlFile::from("qml/Bluetooth.qml").singleton(true))
            .qml_file(QmlFile::from("qml/BluetoothAdapterState.qml").singleton(true))
            .qml_file(QmlFile::from("qml/BluetoothDeviceState.qml").singleton(true)),
    )
    .files(["src/core.rs"])
    .build()
    .export();
}
