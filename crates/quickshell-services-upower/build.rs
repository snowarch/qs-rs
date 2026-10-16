use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Quickshell.Services.UPower")
            .version(0, 1)
            .qml_files(["qml/UPowerDevice.qml"])
            .qml_file(QmlFile::from("qml/UPower.qml").singleton(true))
            .qml_file(QmlFile::from("qml/PowerProfiles.qml").singleton(true))
            .qml_file(QmlFile::from("qml/UPowerDeviceState.qml").singleton(true))
            .qml_file(QmlFile::from("qml/UPowerDeviceType.qml").singleton(true))
            .qml_file(QmlFile::from("qml/PowerProfile.qml").singleton(true))
            .qml_file(QmlFile::from("qml/PerformanceDegradationReason.qml").singleton(true)),
    )
    .files(["src/core.rs"])
    .build()
    .export();
}
