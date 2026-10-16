import QtQml
import Quickshell.Services.Polkit

// upstream PolkitAgent: the agent itself lives in PolkitAgentBase (agent.rs); this builds the
// AuthFlow of the active request and feeds it the conversation events.
PolkitAgentBase {
    id: root
    readonly property AuthFlow flow: _flow
    readonly property bool isActive: _flow !== null
    signal authenticationRequestStarted()

    property AuthFlow _flow: null
    property Component _flowComponent: Component { AuthFlow {} }

    onQsFlowStarted: json => {
        const flow = _flowComponent.createObject(root, { _agent: root })
        flow._init(JSON.parse(json))
        _flow = flow
        authenticationRequestStarted()
    }
    onQsPrompt: (prompt, echo) => _flow?._prompt(prompt, echo)
    onQsSupplementary: (message, error) => _flow?._supplementary(message, error)
    onQsFailed: _flow?._failed()
    onQsFinished: (success, byAgent) => _flow?._finished(success, byAgent)
    onQsFlowEnded: {
        const old = _flow
        _flow = null
        old?.destroy()
    }
    Component.onCompleted: _qsComplete()
}
