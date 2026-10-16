//! `JsonAdapter` / `JsonObject`: a FileView adapter that maps the QML-declared properties of the
//! adapter (and nested JsonObjects) to a JSON document. Reflection lives in include/jsonmeta.h.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;
        include!("quickshell_io/include/jsonmeta.h");
        #[namespace = "qsrs_io"]
        fn json_serialize(adapter: &JsonAdapter) -> QByteArray;
        #[namespace = "qsrs_io"]
        fn json_deserialize(adapter: Pin<&mut JsonAdapter>, data: &QByteArray) -> bool;
        #[namespace = "qsrs_io"]
        fn json_connect_notifiers(adapter: Pin<&mut JsonAdapter>);
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type JsonObject = super::JsonObjectRust;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type JsonAdapter = super::JsonAdapterRust;

        #[qsignal]
        #[cxx_name = "adapterUpdated"]
        fn adapter_updated(self: Pin<&mut JsonAdapter>);

        #[qinvokable]
        #[cxx_name = "onPropertyChanged"]
        fn on_property_changed(self: Pin<&mut JsonAdapter>);
    }

    impl cxx_qt::Initialize for JsonAdapter {}
    impl cxx_qt::Threading for JsonAdapter {}
}

use crate::fileview::qobject::FileViewInternal;
use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QByteArray;

#[derive(Default)]
pub struct JsonObjectRust;

pub struct JsonAdapterRust {
    file_view: *mut FileViewInternal,
    changes_blocked: bool,
}

impl Default for JsonAdapterRust {
    fn default() -> Self {
        Self { file_view: std::ptr::null_mut(), changes_blocked: false }
    }
}

impl cxx_qt::Initialize for qobject::JsonAdapter {
    fn initialize(self: Pin<&mut Self>) {
        // QML-declared properties exist once construction finishes; connect on the next turn.
        let _ = self.qt_thread().queue(|this| qobject::json_connect_notifiers(this));
    }
}

impl qobject::JsonAdapter {
    fn on_property_changed(mut self: Pin<&mut Self>) {
        if self.changes_blocked {
            return;
        }
        qobject::json_connect_notifiers(self.as_mut());
        self.as_mut().adapter_updated();
        if let Some(fv) = unsafe { self.file_view.as_mut() } {
            unsafe { Pin::new_unchecked(fv) }.forward_adapter_updated();
        }
    }
}

/// A FileView adapter resolved from the stored `QObject*`.
pub struct Adapter<'a>(Pin<&'a mut qobject::JsonAdapter>);

impl Adapter<'_> {
    /// # Safety
    /// `ptr` must be null or a live QObject.
    pub unsafe fn from_ptr<'a>(ptr: *mut cxx_qt::QObject) -> Option<Adapter<'a>> {
        use cxx_qt::casting::Downcast;
        let obj = unsafe { Pin::new_unchecked(ptr.as_mut()?) };
        obj.downcast_pin::<qobject::JsonAdapter>().map(Adapter)
    }

    pub fn attach(mut self, view: *mut FileViewInternal) {
        self.0.as_mut().rust_mut().file_view = view;
    }

    pub fn serialize(&self) -> Vec<u8> {
        qobject::json_serialize(&self.0).as_slice().to_vec()
    }

    pub fn deserialize(mut self, data: &[u8]) {
        self.0.as_mut().rust_mut().changes_blocked = true;
        qobject::json_deserialize(self.0.as_mut(), &QByteArray::from(data));
        self.0.as_mut().rust_mut().changes_blocked = false;
        qobject::json_connect_notifiers(self.0.as_mut());
    }
}
