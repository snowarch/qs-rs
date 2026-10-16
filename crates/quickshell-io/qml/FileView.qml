import QtQml
import Quickshell.Io

// Binding-aware wrapper: text() and data() read notifying internal properties, so bindings that
// call them update when the file changes. The default property is the adapter.
FileViewInternal {
    property bool preload: this._qsPreload
    property bool blockLoading: this._qsBlockLoading
    property bool blockAllReads: this._qsBlockAllReads
    property bool printErrors: this._qsPrintErrors
    property string path: this._qsPath
    default property QtObject _qsDefaultAdapter

    on_QsDefaultAdapterChanged: this.adapter = this._qsDefaultAdapter
    onPreloadChanged: this._qsPreload = preload
    onBlockLoadingChanged: this._qsBlockLoading = this.blockLoading
    onBlockAllReadsChanged: this._qsBlockAllReads = this.blockAllReads
    onPrintErrorsChanged: this._qsPrintErrors = this.printErrors

    onPathChanged: {
        if (!this.preload) this._qsPreload = false
        this._qsPrintErrors = this.printErrors
        this._qsPath = this.path
        if (this.preload) this._qsPreload = true
    }

    function sync() {
        if (!this.preload) this._qsPreload = false
        this._qsBlockLoading = this.blockLoading
        this._qsBlockAllReads = this.blockAllReads
        this._qsPrintErrors = this.printErrors
        this._qsPath = this.path
    }

    function text(): string {
        sync()
        this._qsPrepareRead(true)
        const text = this._qsText
        if (this.preload) this._qsPreload = true
        return text
    }

    function data(): var {
        sync()
        this._qsPrepareRead(false)
        const data = this._qsData
        if (this.preload) this._qsPreload = true
        return data
    }
}
