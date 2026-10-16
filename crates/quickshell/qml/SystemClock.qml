import QtQml

// Wall clock that ticks on precision boundaries (whole seconds, minutes or hours), with the
// finer fields zeroed, like quickshell's SystemClock.
QtObject {
    id: root
    enum Enum { Hours = 1, Minutes = 2, Seconds = 3 }
    property bool enabled: true
    property int precision: SystemClock.Seconds
    readonly property date date: _date
    readonly property int hours: _date.getHours()
    readonly property int minutes: _date.getMinutes()
    readonly property int seconds: _date.getSeconds()

    property date _date: new Date()
    property var _target: null

    function _truncate(d) {
        const t = new Date(d)
        t.setMilliseconds(0)
        if (precision < SystemClock.Seconds) t.setSeconds(0)
        if (precision < SystemClock.Minutes) t.setMinutes(0)
        return t
    }
    function _setTime(target) {
        const now = new Date()
        const base = target && Math.abs(target - now) < 500 ? target : now
        _date = _truncate(base)
    }
    function _schedule() {
        const next = _truncate(new Date())
        if (precision >= SystemClock.Seconds) next.setSeconds(next.getSeconds() + 1)
        else if (precision >= SystemClock.Minutes) next.setMinutes(next.getMinutes() + 1)
        else next.setHours(next.getHours() + 1)
        _target = next
        _timer.interval = Math.max(1, next - new Date())
        _timer.restart()
    }
    function _update() {
        if (!enabled) { _timer.stop(); return }
        _setTime(null)
        _schedule()
    }

    property Timer _timer: Timer {
        onTriggered: { root._setTime(root._target); root._schedule() }
    }
    onEnabledChanged: _update()
    onPrecisionChanged: _update()
    Component.onCompleted: _update()
}
