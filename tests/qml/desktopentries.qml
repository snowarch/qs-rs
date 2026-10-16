import QtQuick
import Quickshell
import Quickshell.Io

// DesktopEntries: XDG_DATA_HOME overrides and masks (Hidden) XDG_DATA_DIRS entries, subdirectory
// ids, case-insensitive byId, heuristicLookup by StartupWMClass, NoDisplay kept out of
// applications, actions in Actions= order, execute(), and a rescan when a file appears.
ShellRoot {
    id: root
    property var errors: []
    function check(name, ok) { if (!ok) { errors.push(name); console.log("check failed:", name) } }
    readonly property string dir: Quickshell.env("QSRS_TEST_DIR")
    property int changes: 0

    Connections {
        target: DesktopEntries
        function onApplicationsChanged() { root.changes++ }
    }
    Process { id: writer }
    FileView { id: ran; path: root.dir + "/ran"; printErrors: false }
    Timer { id: poll; interval: 50; repeat: true; property int left: 60; property var test; onTriggered: { if (test()) { stop(); next() } else if (--left == 0) { check("timeout", false); stop(); finish() } } }
    property var next
    function waitFor(test, then) { poll.test = test; poll.left = 60; root.next = then; poll.start() }

    function finish() {
        if (errors.length) { console.log("FAIL", errors.join(", ")); Qt.exit(1) } else { console.log("PASS"); Qt.exit(0) }
    }

    Component.onCompleted: {
        const foo = DesktopEntries.byId("foo");
        check("foo exists", foo !== null);
        check("home overrides sys", foo.name === "Foo home");
        check("unmatched locale ignored", foo.name !== "Wrong");
        check("command", JSON.stringify(foo.command) === JSON.stringify(["sh", "-c", "touch $0", dir + "/ran"]));
        check("working dir", foo.workingDirectory === dir);
        check("terminal", foo.runInTerminal === true);
        check("categories", JSON.stringify(foo.categories) === '["A","B"]');
        check("keywords", JSON.stringify(foo.keywords) === '["k1"]');
        check("actions order", foo.actions.length === 2 && foo.actions[0].id === "second" && foo.actions[1].name === "First");
        check("action icon", foo.actions[0].icon === "sec");
        check("hidden masks", DesktopEntries.byId("hideme") === null);
        check("subdir id", DesktopEntries.byId("kde-sub")?.name === "Sub");
        check("lowercase byId", DesktopEntries.byId("mixed.case")?.name === "Mixed");
        check("exact byId", DesktopEntries.byId("Mixed.Case")?.name === "Mixed");
        check("link skipped", DesktopEntries.byId("link") === null);
        check("heuristic startupClass", DesktopEntries.heuristicLookup("FooWin") === foo);
        check("heuristic lowercase", DesktopEntries.heuristicLookup("foowin") === foo);
        check("heuristic none", DesktopEntries.heuristicLookup("nothing") === null);
        check("nodisplay by id", DesktopEntries.byId("nodisp")?.noDisplay === true);
        const ids = DesktopEntries.applications.values.map(e => e.id).sort().join(",");
        check("applications " + ids, ids === "Mixed.Case,foo,kde-sub");
        foo.execute();
        waitFor(() => root.fileExists(), () => {
            writer.command = ["sh", "-c", `printf '[Desktop Entry]\\nType=Application\\nName=Late\\nExec=true\\n' > "${dir}/home/applications/late.desktop"`];
            writer.running = true;
            waitFor(() => DesktopEntries.byId("late") !== null, () => {
                check("rescan signal", root.changes > 0);
                check("same object kept", DesktopEntries.byId("foo") === foo);
                check("late in applications", DesktopEntries.applications.values.some(e => e.id === "late"));
                finish();
            });
        });
    }
    function fileExists() { ran.reload(); return ran.loaded }
}
