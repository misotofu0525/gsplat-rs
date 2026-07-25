import Foundation

@main
private enum RenderLoopLifecycleTests {
    static func main() {
        let lifecycle = RenderLoopLifecycle()

        require(lifecycle.requestStart(), "an empty lifecycle must allow creation")
        let first = requireToken(lifecycle.begin(), "first loop must install")
        require(first.shouldRun, "a new token must be active")
        require(!lifecycle.requestStart(), "an active loop must not be replaced")
        require(first.shouldRun, "a duplicate start must not cancel the active loop")

        require(!lifecycle.requestReplacement(), "replacement must wait for the old loop")
        require(!first.shouldRun, "replacement must irreversibly cancel the old token")
        require(!lifecycle.requestStart(), "a stopping loop must still block creation")
        require(lifecycle.begin() == nil, "a replacement token must not install before finish")
        require(!first.shouldRun, "a cancelled token must never become active again")

        require(lifecycle.finish(first) == true, "the old completion must release its deferred restart")
        let second = requireToken(lifecycle.begin(), "replacement must install after finish")
        require(second.identity != first.identity, "replacement must have a distinct identity")
        require(second.shouldRun, "replacement token must be active")

        require(lifecycle.finish(first) == nil, "a stale completion must not retire the replacement")
        require(second.shouldRun, "a stale completion must not affect the replacement")
        lifecycle.requestStop()
        lifecycle.requestStop()
        require(!second.shouldRun, "stop must remain irreversible and idempotent")
        require(lifecycle.finish(second) == false, "plain stop must not request a restart")
        require(lifecycle.finish(second) == nil, "duplicate finish must be ignored")

        require(lifecycle.requestReplacement(), "replacement with no loop may start immediately")
        let third = requireToken(lifecycle.begin(), "restart after an empty lifecycle must install")
        lifecycle.requestStop()
        require(lifecycle.finish(third) == false, "a later stop must cancel the pending restart")

        let terminalLifecycle = RenderLoopLifecycle()
        require(terminalLifecycle.requestStart(), "a benchmark loop must start once")
        let terminal = requireToken(
            terminalLifecycle.begin(),
            "the benchmark loop must install"
        )
        require(
            terminalLifecycle.finish(terminal, terminal: true) == false,
            "terminal completion must not request a restart"
        )
        require(
            !terminalLifecycle.requestStart(),
            "layout callbacks after a benchmark terminal must not start a second loop"
        )
        require(
            terminalLifecycle.begin() == nil,
            "a benchmark terminal must permanently seal the render-loop lifecycle"
        )

        print("ios render-loop lifecycle tests ok")
    }

    private static func require(_ condition: @autoclosure () -> Bool, _ message: String) {
        guard condition() else {
            fatalError(message)
        }
    }

    private static func requireToken(
        _ token: RenderLoopLifecycle.Token?,
        _ message: String
    ) -> RenderLoopLifecycle.Token {
        guard let token else {
            fatalError(message)
        }
        return token
    }
}
