import Foundation

/// Coordinates the UIKit-owned lifetime of one native renderer loop at a time.
///
/// The controller and render queue share only a token. Cancellation is
/// irreversible, and a replacement token cannot be installed until the old
/// loop reports that its native handle has been destroyed.
final class RenderLoopLifecycle {
    final class Token {
        let identity: UInt64

        private let lock = NSLock()
        private var cancelled = false

        fileprivate init(identity: UInt64) {
            self.identity = identity
        }

        var shouldRun: Bool {
            lock.lock()
            defer { lock.unlock() }
            return !cancelled
        }

        fileprivate func cancel() {
            lock.lock()
            cancelled = true
            lock.unlock()
        }
    }

    private var nextIdentity: UInt64 = 1
    private var currentToken: Token?
    private var restartAfterStop = false
    private var terminalFinished = false

    /// Returns true only when a renderer may be created immediately.
    /// An already-running loop is left unchanged; a stopping loop records one
    /// deferred restart request.
    func requestStart() -> Bool {
        guard !terminalFinished else {
            return false
        }
        guard let currentToken else {
            return true
        }
        if !currentToken.shouldRun {
            restartAfterStop = true
        }
        return false
    }

    /// Installs the identity for a successfully created native renderer.
    func begin() -> Token? {
        guard currentToken == nil, !terminalFinished else {
            return nil
        }
        let token = Token(identity: nextIdentity)
        precondition(nextIdentity < UInt64.max, "render-loop token identity exhausted")
        nextIdentity += 1
        currentToken = token
        restartAfterStop = false
        return token
    }

    /// Irreversibly stops the current loop and suppresses deferred replacement.
    func requestStop() {
        restartAfterStop = false
        currentToken?.cancel()
    }

    /// Irreversibly stops the current loop and requests a replacement after
    /// its native handle is destroyed. Returns true when no loop is live.
    func requestReplacement() -> Bool {
        restartAfterStop = true
        guard let currentToken else {
            return true
        }
        currentToken.cancel()
        return false
    }

    /// Retires a token after its native handle has been destroyed.
    /// A stale or duplicate completion cannot retire the current loop. A
    /// terminal completion permanently suppresses another loop in this process.
    func finish(_ token: Token, terminal: Bool = false) -> Bool? {
        guard currentToken === token else {
            return nil
        }
        currentToken = nil
        if terminal {
            terminalFinished = true
            restartAfterStop = false
            return false
        }
        let shouldRestart = restartAfterStop
        restartAfterStop = false
        return shouldRestart
    }

    var isRunning: Bool {
        currentToken?.shouldRun == true
    }
}
