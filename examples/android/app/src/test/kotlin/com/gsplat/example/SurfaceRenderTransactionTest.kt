package com.gsplat.example

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.CountDownLatch

class SurfaceRenderTransactionTest {
    @Test
    fun failedCameraCommandDoesNotRequestOrRender() {
        val calls = ArrayList<String>()

        val result = performSurfaceRenderTransaction(
            renderLock = Any(),
            applyCommand = {
                calls += "command"
                17
            },
            closeCurrentStatsOnCommandFailure = { false },
            requestCurrentStats = { calls += "request" },
            stopOnRequestFailure = true,
            render = {
                calls += "render"
                0
            },
            observeRequestedRenderFailure = { calls += "render_failure" },
            reconcileAfterSuccessfulRender = { calls += "reconcile" }
        )

        assertEquals(listOf("command"), calls)
        assertEquals(17, result.rc)
        assertFalse(result.requestAttempted)
        assertNull(result.renderRc)
    }

    @Test
    fun successfulCameraCommandBindsTheDeterminedFrameBeforeRender() {
        val calls = ArrayList<String>()
        var determinedFrame = 0L
        var requestedFrame = 0L

        val result = performSurfaceRenderTransaction(
            renderLock = Any(),
            applyCommand = {
                calls += "command"
                determinedFrame = 42L
                0
            },
            closeCurrentStatsOnCommandFailure = { false },
            requestCurrentStats = {
                calls += "request"
                requestedFrame = determinedFrame
            },
            stopOnRequestFailure = true,
            render = {
                calls += "render"
                assertEquals(determinedFrame, requestedFrame)
                0
            },
            observeRequestedRenderFailure = { calls += "render_failure" },
            reconcileAfterSuccessfulRender = { calls += "submission+poll" }
        )

        assertEquals(listOf("command", "request", "render", "submission+poll"), calls)
        assertTrue(result.requestSucceeded)
        assertTrue(result.reconciliationAttempted)
        assertEquals(0, result.rc)
    }

    @Test
    fun strictRequestFailureDoesNotRenderTheUnobservedFrame() {
        val calls = ArrayList<String>()

        val result = performSurfaceRenderTransaction(
            renderLock = Any(),
            applyCommand = {
                calls += "command"
                0
            },
            closeCurrentStatsOnCommandFailure = { false },
            requestCurrentStats = {
                calls += "request"
                error("request failed")
            },
            stopOnRequestFailure = true,
            render = {
                calls += "render"
                0
            },
            observeRequestedRenderFailure = { calls += "render_failure" },
            reconcileAfterSuccessfulRender = { calls += "reconcile" }
        )

        assertEquals(listOf("command", "request"), calls)
        assertTrue(result.requestAttempted)
        assertFalse(result.requestSucceeded)
        assertNull(result.renderRc)
    }

    @Test
    fun uiRequestFailureStillRendersAndReconcilesNormally() {
        val calls = ArrayList<String>()

        val result = performSurfaceRenderTransaction(
            renderLock = Any(),
            applyCommand = {
                calls += "command"
                0
            },
            closeCurrentStatsOnCommandFailure = { false },
            requestCurrentStats = {
                calls += "request"
                error("request failed")
            },
            stopOnRequestFailure = false,
            render = {
                calls += "render"
                0
            },
            observeRequestedRenderFailure = { calls += "render_failure" },
            reconcileAfterSuccessfulRender = { calls += "submission+poll" }
        )

        assertEquals(listOf("command", "request", "render", "submission+poll"), calls)
        assertTrue(result.requestError is IllegalStateException)
        assertEquals(0, result.rc)
    }

    @Test
    fun resizeCannotEnterBetweenRenderAndSubmissionPoll() {
        val calls = ArrayList<String>()
        val renderLock = Any()
        val renderReached = CountDownLatch(1)
        val resizeAttempting = CountDownLatch(1)
        val resizeThread = Thread {
            renderReached.await()
            resizeAttempting.countDown()
            synchronized(renderLock) {
                calls += "resize"
            }
        }
        resizeThread.start()

        performSurfaceRenderTransaction(
            renderLock = renderLock,
            applyCommand = {
                calls += "command"
                0
            },
            closeCurrentStatsOnCommandFailure = { false },
            requestCurrentStats = { calls += "request" },
            stopOnRequestFailure = true,
            render = {
                calls += "render"
                renderReached.countDown()
                resizeAttempting.await()
                0
            },
            observeRequestedRenderFailure = { calls += "render_failure" },
            reconcileAfterSuccessfulRender = {
                calls += "submission"
                calls += "poll"
            }
        )
        resizeThread.join()

        assertEquals(
            listOf("command", "request", "render", "submission", "poll", "resize"),
            calls
        )
    }

    @Test
    fun commandFailureClosesOutstandingIntentWithoutRequestOrRender() {
        val calls = ArrayList<String>()

        val result = performSurfaceRenderTransaction(
            renderLock = Any(),
            applyCommand = {
                calls += "command"
                23
            },
            closeCurrentStatsOnCommandFailure = {
                calls += "close"
                true
            },
            requestCurrentStats = { calls += "request" },
            stopOnRequestFailure = true,
            render = {
                calls += "render"
                0
            },
            observeRequestedRenderFailure = { calls += "render_failure" },
            reconcileAfterSuccessfulRender = { calls += "reconcile" }
        )

        assertEquals(listOf("command", "close"), calls)
        assertTrue(result.commandFailureClosedCurrentStats)
        assertEquals(23, result.rc)
    }
}
