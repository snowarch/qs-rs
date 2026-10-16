import QtQml

// upstream AuthFlow (services/polkit/flow.hpp). Identities are plain objects with the members of
// upstream's Identity: id, string, displayName, isGroup.
QtObject {
    id: flow
    property string message
    property string iconName
    property string actionId
    property string cookie
    readonly property var identities: _identities
    property var selectedIdentity: null
    readonly property bool isResponseRequired: _isResponseRequired
    readonly property string inputPrompt: _inputPrompt
    readonly property bool responseVisible: _responseVisible
    readonly property string supplementaryMessage: _supplementaryMessage
    readonly property bool supplementaryIsError: _supplementaryIsError
    readonly property bool isCompleted: _isCompleted
    readonly property bool isSuccessful: _isSuccessful
    readonly property bool isCancelled: _isCancelled
    readonly property bool failed: _failedOnce

    signal authenticationSucceeded()
    signal authenticationFailed()
    signal authenticationRequestCancelled()

    function submit(value) {
        if (_isCompleted) return
        _agent._qsSubmit(value)
        _isResponseRequired = false
        _inputPrompt = ""
        _responseVisible = false
    }
    function cancelAuthenticationRequest() {
        if (_isCompleted) return
        _agent._qsCancel()
    }

    property QtObject _agent
    property var _identities: []
    property bool _isResponseRequired
    property string _inputPrompt
    property bool _responseVisible
    property string _supplementaryMessage
    property bool _supplementaryIsError
    property bool _isCompleted
    property bool _isSuccessful
    property bool _isCancelled
    property bool _failedOnce
    property var _lastIdentity: null

    function _init(d) {
        message = d.message
        iconName = d.iconName
        actionId = d.actionId
        cookie = d.cookie
        _identities = d.identities
        _lastIdentity = _identities[0]
        selectedIdentity = _lastIdentity
    }
    onSelectedIdentityChanged: {
        if (selectedIdentity === _lastIdentity) return
        const i = _identities.indexOf(selectedIdentity)
        if (i < 0) {
            console.warn("AuthFlow: Cannot set selected identity to null.")
            selectedIdentity = _lastIdentity
            return
        }
        _lastIdentity = selectedIdentity
        _agent._qsSelectIdentity(i)
    }
    function _prompt(prompt, echo) {
        _isResponseRequired = true
        _inputPrompt = prompt
        _responseVisible = echo
    }
    function _supplementary(text, error) {
        _supplementaryIsError = error
        _supplementaryMessage = text
    }
    function _failed() {
        _failedOnce = true
        authenticationFailed()
    }
    function _finished(success, byAgent) {
        _isResponseRequired = false
        _isCancelled = !success
        _isSuccessful = success
        _isCompleted = true
        if (success) authenticationSucceeded()
        else if (byAgent) authenticationRequestCancelled()
    }
}
