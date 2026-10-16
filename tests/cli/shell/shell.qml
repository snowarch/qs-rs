//@ pragma ShellId clitest
import QtQml
import Quickshell
import Quickshell.Io

Scope {
    IpcHandler {
        target: "cli"
        property string mode: "ready"
        function echo(text: string): string { return "echo:" + text }
    }
}
