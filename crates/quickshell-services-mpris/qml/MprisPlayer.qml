import QtQml
import Quickshell.Services.Mpris

// One MPRIS player. `position` is computed from the last reported position and the time since,
// like quickshell: emit positionChanged() to re-read it. Writable properties are sent to the
// player; what it confirms comes back through PropertiesChanged.
QtObject {
    id: root
    property string dbusName
    property bool ready: false

    property bool canControl: false
    property bool _canPlay: false
    property bool _canPause: false
    property bool _canSeek: false
    property bool _canGoNext: false
    property bool _canGoPrevious: false
    readonly property bool canPlay: canControl && _canPlay
    readonly property bool canPause: canControl && _canPause
    readonly property bool canSeek: canControl && _canSeek
    readonly property bool canGoNext: canControl && _canGoNext
    readonly property bool canGoPrevious: canControl && _canGoPrevious
    readonly property bool canTogglePlaying: isPlaying ? canPause : canPlay
    property bool canQuit: false
    property bool canRaise: false
    property bool canSetFullscreen: false
    property string identity
    property string desktopEntry

    property real position: 0
    property bool positionSupported: false
    readonly property real length: lengthSupported ? Math.floor(_length / 1000) / 1000 : position
    readonly property bool lengthSupported: _length !== -1
    property real volume: 1
    property bool volumeSupported: false

    property var metadata: ({})
    property int uniqueId: 0
    readonly property string trackTitle: metadata["xesam:title"] ?? ""
    readonly property string trackArtist: (metadata["xesam:artist"] ?? []).join(", ")
    readonly property string trackArtists: trackArtist
    readonly property string trackAlbum: metadata["xesam:album"] ?? ""
    readonly property string trackAlbumArtist: String(metadata["xesam:albumArtist"] ?? "")
    readonly property string trackArtUrl: metadata["mpris:artUrl"] ?? ""

    property int playbackState: MprisPlaybackState.Stopped
    property bool isPlaying: false
    property int loopState: MprisLoopState.None
    property bool loopSupported: false
    property real rate: 1
    property real minRate: 1
    property real maxRate: 1
    property bool shuffle: false
    property bool shuffleSupported: false
    property bool fullscreen: false
    property list<string> supportedUriSchemes
    property list<string> supportedMimeTypes

    signal trackChanged()
    signal postTrackChanged()

    readonly property string _app: "org.mpris.MediaPlayer2"
    readonly property string _player: "org.mpris.MediaPlayer2.Player"
    property real _length: -1
    property real _positionUs: -1
    property real _stamp: 0
    property real _pausedStamp: 0
    property string _trackId
    property string _trackUrl
    property string _trackTitle
    property bool _trackChangedBeforeState: false
    property bool _syncing: false
    property bool _inPosition: false
    property real _shownPosition: 0
    property var _seen: ({})

    function _positionMs() {
        if (!positionSupported || playbackState === MprisPlaybackState.Stopped) return 0
        const time = playbackState === MprisPlaybackState.Paused ? _pausedStamp : Date.now()
        return Math.floor(_positionUs / 1000) + Math.floor((time - _stamp) * Math.floor(rate * 1000) / 1000)
    }
    function _showPosition() {
        _inPosition = true
        _shownPosition = _positionMs() / 1000
        position = _shownPosition
        _inPosition = false
    }
    function _setPositionUs(us) {
        _positionUs = us
        _stamp = Date.now()
        _pausedStamp = _stamp
        positionSupported = true
        _showPosition()
        positionChanged()
    }
    // positionChanged() from outside re-reads the clock; a different value written from
    // outside is a seek request.
    onPositionChanged: {
        if (_inPosition) return
        if (position !== _shownPosition) {
            const target = position
            _setPosition(target)
            return
        }
        _showPosition()
    }
    function _setPosition(seconds) {
        if (_positionUs === -1) { console.warn("Cannot set position of", identity, "because position is not supported."); return _showPosition() }
        if (!canSeek) { console.warn("Cannot set position of", identity, "because canSeek is false."); return _showPosition() }
        const target = Math.floor(seconds * 1000) * 1000
        if (_trackId) MprisCore.setPosition(dbusName, _trackId, target)
        else MprisCore.seek(dbusName, target - _positionMs() * 1000)
        _setPositionUs(target)
    }

    function raise() { if (!canRaise) return console.warn("Cannot call raise() on", identity, "because canRaise is false."); MprisCore.call(dbusName, _app, "Raise") }
    function quit() { if (!canQuit) return console.warn("Cannot call quit() on", identity, "because canQuit is false."); MprisCore.call(dbusName, _app, "Quit") }
    function openUri(uri) { MprisCore.openUri(dbusName, uri) }
    function next() { if (!canGoNext) return console.warn("Cannot call next() on", identity, "because canGoNext is false."); MprisCore.call(dbusName, _player, "Next") }
    function previous() { if (!canGoPrevious) return console.warn("Cannot call previous() on", identity, "because canGoPrevious is false."); MprisCore.call(dbusName, _player, "Previous") }
    function seek(offset) { if (!canSeek) return console.warn("Cannot call seek() on", identity, "because canSeek is false."); MprisCore.seek(dbusName, Math.floor(offset * 1000) * 1000) }
    function play() { _requestState(MprisPlaybackState.Playing) }
    function pause() { _requestState(MprisPlaybackState.Paused) }
    function stop() { _requestState(MprisPlaybackState.Stopped) }
    function togglePlaying() { if (isPlaying) pause(); else play() }

    function _requestState(state) {
        if (state === MprisPlaybackState.Stopped && canControl) MprisCore.call(dbusName, _player, "Stop")
        else if (state === MprisPlaybackState.Playing && canPlay) MprisCore.call(dbusName, _player, "Play")
        else if (state === MprisPlaybackState.Paused && canPause) MprisCore.call(dbusName, _player, "Pause")
        else console.warn("Cannot set playbackState of", identity, "to", MprisPlaybackState.toString(state))
    }
    function _restore(f) { _syncing = true; f(); _syncing = false }

    onPlaybackStateChanged: if (!_syncing) { const want = playbackState; _restore(() => playbackState = _seen.state ?? 0); _requestState(want) }
    onIsPlayingChanged: if (!_syncing) { const want = isPlaying; _restore(() => isPlaying = !want); if (want) play(); else pause() }
    onVolumeChanged: if (!_syncing) {
        if (!canControl || !volumeSupported) { console.warn("Cannot set volume of", identity); return _restore(() => volume = _seen.Volume ?? 1) }
        MprisCore.setDouble(dbusName, _player, "Volume", volume)
    }
    onLoopStateChanged: if (!_syncing) {
        if (!canControl || !loopSupported) { console.warn("Cannot set loopState of", identity); return _restore(() => loopState = _seen.loop ?? 0) }
        MprisCore.setString(dbusName, _player, "LoopStatus", MprisLoopState._wire[loopState] ?? "None")
    }
    onRateChanged: if (!_syncing) {
        if (rate < minRate || rate > maxRate) { console.warn("Cannot set rate of", identity, "outside minRate and maxRate"); return _restore(() => rate = _seen.Rate ?? 1) }
        MprisCore.setDouble(dbusName, _player, "Rate", rate)
    }
    onShuffleChanged: if (!_syncing) {
        if (!canControl || !shuffleSupported) { console.warn("Cannot set shuffle of", identity); return _restore(() => shuffle = _seen.Shuffle ?? false) }
        MprisCore.setBool(dbusName, _player, "Shuffle", shuffle)
    }
    onFullscreenChanged: if (!_syncing) {
        if (!canSetFullscreen) { console.warn("Cannot set fullscreen of", identity); return _restore(() => fullscreen = _seen.Fullscreen ?? false) }
        MprisCore.setBool(dbusName, _app, "Fullscreen", fullscreen)
    }

    function _metadata(m) {
        const trackId = String(m["mpris:trackid"] ?? "")
        const url = m["xesam:url"], title = m["xesam:title"]
        let changed = trackId !== _trackId
        _trackId = trackId
        if (url !== undefined && String(url) !== _trackUrl) { _trackUrl = String(url); changed = true }
        if (title !== undefined && String(title) !== _trackTitle) { _trackTitle = String(title); changed = true }
        if (changed) {
            trackChanged()
            uniqueId++
            _trackChangedBeforeState = true
            MprisCore.refresh(dbusName, _player, "Position")
        }
        const len = m["mpris:length"]
        _length = typeof len === "number" ? len : -1
        metadata = m
        if (changed) postTrackChanged()
        if (!lengthSupported) positionChanged()
    }

    function _update(iface, p) {
        _syncing = true
        if (iface === _app) {
            if (p.Identity !== undefined) identity = p.Identity
            if (p.DesktopEntry !== undefined) desktopEntry = p.DesktopEntry
            if (p.CanQuit !== undefined) canQuit = p.CanQuit
            if (p.CanRaise !== undefined) canRaise = p.CanRaise
            if (p.CanSetFullscreen !== undefined) canSetFullscreen = p.CanSetFullscreen
            if (p.Fullscreen !== undefined) { _seen.Fullscreen = p.Fullscreen; fullscreen = p.Fullscreen }
            if (p.SupportedUriSchemes !== undefined) supportedUriSchemes = p.SupportedUriSchemes
            if (p.SupportedMimeTypes !== undefined) supportedMimeTypes = p.SupportedMimeTypes
        } else if (iface === _player) {
            if (p.CanControl !== undefined) canControl = p.CanControl
            if (p.CanPlay !== undefined) _canPlay = p.CanPlay
            if (p.CanPause !== undefined) _canPause = p.CanPause
            if (p.CanSeek !== undefined) _canSeek = p.CanSeek
            if (p.CanGoNext !== undefined) _canGoNext = p.CanGoNext
            if (p.CanGoPrevious !== undefined) _canGoPrevious = p.CanGoPrevious
            if (p.Volume !== undefined) { _seen.Volume = p.Volume; volume = p.Volume; volumeSupported = true }
            if (p.LoopStatus !== undefined) { loopState = Math.max(0, MprisLoopState._wire.indexOf(p.LoopStatus)); _seen.loop = loopState; loopSupported = true }
            if (p.Rate !== undefined) { _seen.Rate = p.Rate; rate = p.Rate }
            if (p.MinimumRate !== undefined) minRate = p.MinimumRate
            if (p.MaximumRate !== undefined) maxRate = p.MaximumRate
            if (p.Shuffle !== undefined) { _seen.Shuffle = p.Shuffle; shuffle = p.Shuffle; shuffleSupported = true }
            if (p.Metadata !== undefined) _metadata(p.Metadata)
            if (p.PlaybackStatus !== undefined) {
                const state = MprisPlaybackState._fromWire(p.PlaybackStatus)
                _seen.state = state
                if (state === MprisPlaybackState.Paused && playbackState === MprisPlaybackState.Playing) _pausedStamp = Date.now()
                playbackState = state
                isPlaying = state === MprisPlaybackState.Playing
                MprisCore.refresh(dbusName, _player, "Position")
                _refreshLater.restart()
                if (_trackChangedBeforeState) { _trackChangedBeforeState = false; MprisCore.refresh(dbusName, _player, "Metadata") }
            }
            if (p.Position !== undefined) { _syncing = false; _setPositionUs(p.Position); _syncing = true }
        }
        _syncing = false
    }

    property Timer _refreshLater: Timer { interval: 100; onTriggered: MprisCore.refresh(root.dbusName, root._player, "Position") }
}
