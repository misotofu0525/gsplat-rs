package com.gsplat.example

import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SurfaceRenderSessionOwnerTest {
    @Test
    fun stoppedOwnerRetainsSlotUntilBlockedThreadDestroysItsOwnHandle() {
        val owner = SurfaceRenderSessionOwner(Any())
        val sessionA = (owner.reserve() as SurfaceRenderSessionOwner.ReserveResult.Acquired)
            .session
        val enteredNativeCall = CountDownLatch(1)
        val interruptObserved = CountDownLatch(1)
        val releaseNativeCall = CountDownLatch(1)
        val destroyedHandles = Collections.synchronizedList(mutableListOf<Long>())

        val threadA = Thread {
            check(owner.publishHandle(sessionA, 11L))
            enteredNativeCall.countDown()
            while (true) {
                try {
                    releaseNativeCall.await()
                    break
                } catch (_: InterruptedException) {
                    // Model a JNI create/render call that outlives the old
                    // one-second interrupt/join window.
                    interruptObserved.countDown()
                }
            }
            destroyedHandles += 11L
            check(owner.clearOwnedHandle(sessionA, 11L))
            check(owner.finish(sessionA, Thread.currentThread()))
        }
        assertTrue(owner.attachThread(sessionA, threadA))
        threadA.start()
        assertTrue(enteredNativeCall.await(5, TimeUnit.SECONDS))

        val stopResult = owner.requestStopAndAwait(0L)
        assertEquals(
            SurfaceRenderSessionOwner.StopResult.Retiring(sessionA.generation),
            stopResult
        )
        assertTrue(interruptObserved.await(5, TimeUnit.SECONDS))

        // B cannot reuse the slot while A is still inside the native call.
        assertEquals(
            SurfaceRenderSessionOwner.ReserveResult.Busy(
                generation = sessionA.generation,
                retiring = true
            ),
            owner.reserve()
        )
        assertEquals(11L, owner.currentHandle())
        assertEquals(0L, owner.activeHandle())

        releaseNativeCall.countDown()
        threadA.join()
        assertEquals(listOf(11L), destroyedHandles)

        val sessionB = (owner.reserve() as SurfaceRenderSessionOwner.ReserveResult.Acquired)
            .session
        val threadB = Thread.currentThread()
        assertTrue(owner.attachThread(sessionB, threadB))
        assertTrue(owner.publishHandle(sessionB, 22L))

        // Even a stale A cleanup attempt cannot clear or finish B.
        assertFalse(owner.clearOwnedHandle(sessionA, 11L))
        assertFalse(owner.finish(sessionA, threadA))
        assertEquals(22L, owner.currentHandle())

        destroyedHandles += 22L
        assertTrue(owner.clearOwnedHandle(sessionB, 22L))
        assertTrue(owner.finish(sessionB, threadB))
        assertEquals(listOf(11L, 22L), destroyedHandles)
    }
}
