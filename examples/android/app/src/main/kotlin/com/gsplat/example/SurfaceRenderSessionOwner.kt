package com.gsplat.example

/**
 * Owns the one Android render-thread slot and its native handle publication.
 *
 * A stopped session keeps the slot until its owner thread has destroyed its
 * handle and called [finish]. A bounded lifecycle wait may therefore return
 * [StopResult.Retiring], but no replacement session can start. If a native
 * call never returns, this process remains fail-closed with that one retiring
 * owner instead of racing a destroy or accumulating more sessions.
 */
internal class SurfaceRenderSessionOwner(private val lock: Any) {
    internal sealed interface ReserveResult {
        data class Acquired(val session: Session) : ReserveResult
        data class Busy(val generation: Long, val retiring: Boolean) : ReserveResult
    }

    internal sealed interface StopResult {
        data object Idle : StopResult
        data class Stopped(val generation: Long) : StopResult
        data class Retiring(val generation: Long) : StopResult
    }

    internal class Session internal constructor(val generation: Long) {
        @Volatile
        internal var stopRequested = false
    }

    private data class Slot(
        val session: Session,
        var thread: Thread? = null,
        var handle: Long = 0L
    )

    private var nextGeneration = 1L
    private var slot: Slot? = null

    fun reserve(): ReserveResult = synchronized(lock) {
        slot?.let { current ->
            return@synchronized ReserveResult.Busy(
                generation = current.session.generation,
                retiring = current.session.stopRequested
            )
        }
        val session = Session(nextGeneration++)
        slot = Slot(session)
        ReserveResult.Acquired(session)
    }

    fun attachThread(session: Session, thread: Thread): Boolean = synchronized(lock) {
        val current = slot
        if (
            current?.session !== session || current.thread != null ||
            session.stopRequested
        ) {
            return@synchronized false
        }
        current.thread = thread
        true
    }

    fun publishHandle(session: Session, handle: Long): Boolean = synchronized(lock) {
        require(handle != 0L) { "native renderer handle must be non-zero" }
        val current = slot
        if (
            current?.session !== session || current.handle != 0L ||
            session.stopRequested
        ) {
            return@synchronized false
        }
        current.handle = handle
        true
    }

    fun currentHandle(): Long = synchronized(lock) { slot?.handle ?: 0L }

    fun activeHandle(): Long = synchronized(lock) {
        slot?.takeUnless { it.session.stopRequested }?.handle ?: 0L
    }

    fun clearOwnedHandle(session: Session, handle: Long): Boolean = synchronized(lock) {
        val current = slot
        if (current?.session !== session || current.handle != handle) {
            return@synchronized false
        }
        current.handle = 0L
        true
    }

    fun shouldRun(session: Session): Boolean = !session.stopRequested

    fun requestStop(session: Session): Boolean = synchronized(lock) {
        if (slot?.session !== session) return@synchronized false
        session.stopRequested = true
        true
    }

    /** Requests shutdown and retains the slot if the bounded wait expires. */
    fun requestStopAndAwait(timeoutMillis: Long): StopResult {
        require(timeoutMillis >= 0L) { "shutdown timeout must be non-negative" }
        val (session, thread) = synchronized(lock) {
            val current = slot ?: return StopResult.Idle
            current.session.stopRequested = true
            current.session to checkNotNull(current.thread) {
                "reserved render session has no owner thread"
            }
        }
        check(thread !== Thread.currentThread()) {
            "render owner thread cannot join itself"
        }
        thread.interrupt()

        var callerInterrupted = false
        if (timeoutMillis > 0L && thread.isAlive) {
            try {
                thread.join(timeoutMillis)
            } catch (_: InterruptedException) {
                callerInterrupted = true
                thread.interrupt()
            }
        }
        if (callerInterrupted) {
            Thread.currentThread().interrupt()
        }
        if (thread.isAlive) {
            return StopResult.Retiring(session.generation)
        }
        check(synchronized(lock) { slot?.session !== session }) {
            "render owner exited without releasing generation ${session.generation}"
        }
        return StopResult.Stopped(session.generation)
    }

    /** Only the attached owner thread may release its generation. */
    fun finish(session: Session, thread: Thread): Boolean = synchronized(lock) {
        val current = slot
        if (current?.session !== session || current.thread !== thread) {
            return@synchronized false
        }
        check(current.handle == 0L) {
            "render generation ${session.generation} finished with a live native handle"
        }
        session.stopRequested = true
        slot = null
        true
    }
}
