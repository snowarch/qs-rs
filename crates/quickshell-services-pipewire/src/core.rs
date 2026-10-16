//! `PwCore`: owns the PipeWire thread. Backend updates are queued to the Qt thread and re-emitted
//! as signals; the QML layer (Pipewire.qml) turns them into one object per node and link.
//! Commands go back to the PipeWire loop through its channel.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qhash.h");
        type QHash_QString_QVariant = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_QString_QVariant>;
        include!("cxx-qt-lib/qlist.h");
        type QList_f64 = cxx_qt_lib::QList<f64>;
        type QList_i32 = cxx_qt_lib::QList<i32>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element = "PwCore"]
        #[qml_singleton]
        #[qproperty(bool, ready, READ, NOTIFY = ready_changed)]
        type PwCore = super::PwCoreRust;

        #[qinvokable]
        #[cxx_name = "setVolumes"]
        fn set_volumes(self: &PwCore, node: i32, volumes: &QList_f64);
        #[qinvokable]
        #[cxx_name = "setMuted"]
        fn set_muted(self: &PwCore, node: i32, muted: bool);
        #[qinvokable]
        #[cxx_name = "setDefault"]
        fn set_default(self: &PwCore, key: &QString, name: &QString);

        #[qsignal]
        #[cxx_name = "readyChanged"]
        fn ready_changed(self: Pin<&mut PwCore>);
        #[qsignal]
        #[cxx_name = "nodeChanged"]
        fn node_changed(self: Pin<&mut PwCore>, id: i32, properties: QHash_QString_QVariant);
        #[qsignal]
        #[cxx_name = "nodeAudioChanged"]
        fn node_audio_changed(self: Pin<&mut PwCore>, id: i32, volumes: QList_f64, channels: QList_i32, muted: bool);
        #[qsignal]
        #[cxx_name = "nodeRemoved"]
        fn node_removed(self: Pin<&mut PwCore>, id: i32);
        #[qsignal]
        #[cxx_name = "linkChanged"]
        fn link_changed(self: Pin<&mut PwCore>, id: i32, output_node: i32, input_node: i32, state: i32);
        #[qsignal]
        #[cxx_name = "linkRemoved"]
        fn link_removed(self: Pin<&mut PwCore>, id: i32);
        #[qsignal]
        #[cxx_name = "defaultChanged"]
        fn default_changed(self: Pin<&mut PwCore>, key: QString, name: QString);
    }

    impl cxx_qt::Initialize for PwCore {}
    impl cxx_qt::Threading for PwCore {}
}

use crate::backend::{self, Command, Update};
use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QHash, QHashPair_QString_QVariant, QList, QString, QVariant};
use pipewire as pw;

#[derive(Default)]
pub struct PwCoreRust {
    ready: bool,
    commands: Option<pw::channel::Sender<Command>>,
}

impl cxx_qt::Initialize for qobject::PwCore {
    fn initialize(mut self: Pin<&mut Self>) {
        let (tx, rx) = pw::channel::channel::<Command>();
        self.as_mut().rust_mut().commands = Some(tx);
        let qt = self.qt_thread();
        std::thread::Builder::new()
            .name("pipewire".into())
            .spawn(move || {
                let emit = move |update: Update| {
                    let _ = qt.queue(move |this| deliver(this, update));
                };
                if let Err(e) = backend::run(emit, rx) {
                    quickshell::qs_log!(warn, "quickshell.service.pipewire", "could not connect: {e}");
                }
            })
            .expect("spawn pipewire thread");
    }
}

fn deliver(mut this: Pin<&mut qobject::PwCore>, update: Update) {
    match update {
        Update::Node { id, props } => {
            let mut hash = QHash::<QHashPair_QString_QVariant>::default();
            for (k, v) in props {
                hash.insert(QString::from(&k), QVariant::from(&QString::from(&v)));
            }
            this.node_changed(id as i32, hash);
        }
        Update::NodeAudio { id, volumes, channels, muted } => {
            let mut vols = QList::<f64>::default();
            for v in volumes {
                vols.append(v as f64);
            }
            let mut chans = QList::<i32>::default();
            for c in channels {
                chans.append(c as i32);
            }
            this.node_audio_changed(id as i32, vols, chans, muted);
        }
        Update::NodeRemoved(id) => this.node_removed(id as i32),
        Update::Link { id, output_node, input_node, state } => {
            this.link_changed(id as i32, output_node as i32, input_node as i32, state)
        }
        Update::LinkRemoved(id) => this.link_removed(id as i32),
        Update::Default { key, name } => this.default_changed(QString::from(&key), QString::from(&name)),
        Update::Ready => {
            this.as_mut().rust_mut().ready = true;
            this.ready_changed();
        }
    }
}

impl qobject::PwCore {
    fn send(&self, cmd: Command) {
        if let Some(tx) = &self.commands {
            let _ = tx.send(cmd);
        }
    }

    fn set_volumes(&self, node: i32, volumes: &QList<f64>) {
        self.send(Command::SetVolumes { node: node as u32, volumes: volumes.iter().map(|v| *v as f32).collect() });
    }

    fn set_muted(&self, node: i32, muted: bool) {
        self.send(Command::SetMuted { node: node as u32, muted });
    }

    fn set_default(&self, key: &QString, name: &QString) {
        self.send(Command::SetDefault { key: key.to_string(), name: name.to_string() });
    }
}
